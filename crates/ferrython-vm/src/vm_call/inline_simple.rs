use compact_str::CompactString;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{
    has_descriptor_get, CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_GETATTR, CLASS_FLAG_HAS_GETATTRIBUTE,
};
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
                Opcode::LoadFastLoadAttr => {
                    let local_idx = (instrs[0].arg >> 16) as usize;
                    let name_idx = (instrs[0].arg & 0xFFFF) as usize;
                    if local_idx != 0 {
                        return None;
                    }
                    let attr = inline_instance_attr(pyfunc, arg, name_idx)?;
                    Self::fast_binary_result(instrs[1].op, &attr, arg)
                }
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
            6 if instrs[5].op == Opcode::ReturnValue => {
                try_inline_attr_binary_attr(pyfunc, arg, instrs)
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
            5 if instrs[4].op == Opcode::ReturnValue => {
                try_inline_two_arg_attr_compare(pyfunc, args, instrs)
            }
            18 => try_inline_two_attr_eq_with_attribute_error_false(pyfunc, args, instrs),
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
fn inline_instance_attr(
    pyfunc: &PyFunction,
    obj: &PyObjectRef,
    name_idx: usize,
) -> Option<PyObjectRef> {
    inline_instance_attr_result(pyfunc, obj, name_idx).into_value()
}

#[inline(always)]
fn inline_instance_attr_result(
    pyfunc: &PyFunction,
    obj: &PyObjectRef,
    name_idx: usize,
) -> InlineAttrResult {
    let Some(name) = pyfunc.code.names.get(name_idx) else {
        return InlineAttrResult::Unsafe;
    };
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return InlineAttrResult::Unsafe;
    };
    if inst.class_flags & (CLASS_FLAG_HAS_GETATTRIBUTE | CLASS_FLAG_HAS_GETATTR) != 0
        || inst.dict_storage.is_some()
    {
        return InlineAttrResult::Unsafe;
    }
    let attrs = unsafe { &*inst.attrs.data_ptr() };
    if inst.is_special
        || attrs.contains_key("__deque__")
        || attrs.contains_key("__builtin_value__")
        || attrs.contains_key("__class__")
    {
        return InlineAttrResult::Unsafe;
    }
    if let Some(value) = attrs.get(name.as_str()) {
        return InlineAttrResult::Value(value.clone());
    }
    if inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0 {
        return InlineAttrResult::Unsafe;
    }
    let PyObjectPayload::Class(cd) = &inst.class.payload else {
        return InlineAttrResult::Unsafe;
    };
    let vtable = unsafe { &*cd.method_vtable.data_ptr() };
    if vtable.is_empty() {
        return InlineAttrResult::Missing;
    }
    let Some(class_value) = vtable.get(name.as_str()) else {
        return InlineAttrResult::Missing;
    };
    match &class_value.payload {
        PyObjectPayload::Function(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure { .. }
        | PyObjectPayload::Property(_)
        | PyObjectPayload::ClassMethod(_)
        | PyObjectPayload::StaticMethod(_) => InlineAttrResult::Unsafe,
        _ if has_descriptor_get(class_value) => InlineAttrResult::Unsafe,
        _ => InlineAttrResult::Value(class_value.clone()),
    }
}

#[inline(always)]
fn try_inline_attr_binary_attr(
    pyfunc: &PyFunction,
    arg: &PyObjectRef,
    instrs: &[Instruction],
) -> Option<PyObjectRef> {
    let (left_local, left_name) = unpack_local_name(instrs[0])?;
    let (right_local, right_name) = unpack_local_name(instrs[3])?;
    if left_local != 0 || right_local != 0 {
        return None;
    }
    let left = inline_instance_attr(pyfunc, arg, left_name)?;
    let right = match instrs[1].op {
        Opcode::LoadConst => pyfunc.constant_cache.get(instrs[1].arg as usize)?.clone(),
        _ => return None,
    };
    let first = VirtualMachine::fast_binary_result(instrs[2].op, &left, &right)?;
    let second = inline_instance_attr(pyfunc, arg, right_name)?;
    VirtualMachine::fast_binary_result(instrs[4].op, &first, &second)
}

#[inline(always)]
fn try_inline_two_arg_attr_compare(
    pyfunc: &PyFunction,
    args: &[PyObjectRef],
    instrs: &[Instruction],
) -> Option<PyObjectRef> {
    let (left_obj_idx, left_name_idx) = unpack_local_name(instrs[0])?;
    let (right_obj_idx, right_name_idx) = unpack_local_name(instrs[1])?;
    if instrs[2].op != Opcode::CompareOp || instrs[2].arg != 2 {
        return None;
    }
    let left = inline_instance_attr(pyfunc, args.get(left_obj_idx)?, left_name_idx)?;
    let right = inline_instance_attr(pyfunc, args.get(right_obj_idx)?, right_name_idx)?;
    Some(PyObject::bool_val(
        left.compare(&right, CompareOp::Eq).ok()?.is_truthy(),
    ))
}

#[inline(always)]
fn try_inline_two_attr_eq_with_attribute_error_false(
    pyfunc: &PyFunction,
    args: &[PyObjectRef],
    instrs: &[Instruction],
) -> Option<PyObjectRef> {
    if args.len() != 2
        || pyfunc.code.arg_count != 2
        || instrs[0].op != Opcode::SetupExcept
        || instrs[4].op != Opcode::JumpIfFalseOrPop
        || instrs[4].arg != 8
        || instrs[8].op != Opcode::ReturnValue
        || instrs[9].op != Opcode::DupTop
        || instrs[10].op != Opcode::LoadGlobal
        || pyfunc
            .code
            .names
            .get(instrs[10].arg as usize)
            .map(|name| name.as_str())
            != Some("AttributeError")
        || instrs[11].op != Opcode::CompareOpPopJumpIfFalse
        || instrs[12].op != Opcode::PopTop
        || instrs[13].op != Opcode::PopTop
        || instrs[14].op != Opcode::PopTop
        || instrs[15].op != Opcode::LoadConstReturnValue
        || instrs[16].op != Opcode::EndFinally
        || instrs[17].op != Opcode::LoadConstReturnValue
    {
        return None;
    }
    let jump_cmp_op = instrs[11].arg >> 24;
    if jump_cmp_op != 10 {
        return None;
    }
    let false_value = pyfunc.constant_cache.get(instrs[15].arg as usize)?;
    let trailing_value = pyfunc.constant_cache.get(instrs[17].arg as usize)?;
    if !matches!(&false_value.payload, PyObjectPayload::Bool(false))
        || !matches!(&trailing_value.payload, PyObjectPayload::None)
    {
        return None;
    }

    let first = inline_attr_eq(pyfunc, args, instrs[1], instrs[2], instrs[3])?;
    if !first {
        return Some(PyObject::bool_val(false));
    }
    Some(PyObject::bool_val(inline_attr_eq(
        pyfunc, args, instrs[5], instrs[6], instrs[7],
    )?))
}

#[inline(always)]
fn inline_attr_eq(
    pyfunc: &PyFunction,
    args: &[PyObjectRef],
    left_instr: Instruction,
    right_instr: Instruction,
    cmp_instr: Instruction,
) -> Option<bool> {
    if cmp_instr.op != Opcode::CompareOp || cmp_instr.arg != 2 {
        return None;
    }
    let (left_obj_idx, left_name_idx) = unpack_local_name(left_instr)?;
    let (right_obj_idx, right_name_idx) = unpack_local_name(right_instr)?;
    let left = inline_instance_attr_result(pyfunc, args.get(left_obj_idx)?, left_name_idx);
    let right = inline_instance_attr_result(pyfunc, args.get(right_obj_idx)?, right_name_idx);
    match (left, right) {
        (InlineAttrResult::Value(left), InlineAttrResult::Value(right)) => {
            if let Some(result) = inline_eq_bool(&left, &right) {
                Some(result)
            } else {
                Some(left.compare(&right, CompareOp::Eq).ok()?.is_truthy())
            }
        }
        (InlineAttrResult::Missing, _) | (_, InlineAttrResult::Missing) => Some(false),
        _ => None,
    }
}

#[inline(always)]
fn inline_eq_bool(left: &PyObjectRef, right: &PyObjectRef) -> Option<bool> {
    match (&left.payload, &right.payload) {
        (PyObjectPayload::Int(left), PyObjectPayload::Int(right)) => Some(left == right),
        (PyObjectPayload::Bool(left), PyObjectPayload::Bool(right)) => Some(left == right),
        (PyObjectPayload::Bool(left), PyObjectPayload::Int(PyInt::Small(right)))
        | (PyObjectPayload::Int(PyInt::Small(right)), PyObjectPayload::Bool(left)) => {
            Some((*left as i64) == *right)
        }
        (PyObjectPayload::Float(left), PyObjectPayload::Float(right)) => Some(left == right),
        (PyObjectPayload::Str(left), PyObjectPayload::Str(right)) => Some(left == right),
        (PyObjectPayload::None, PyObjectPayload::None) => Some(true),
        _ => None,
    }
}

enum InlineAttrResult {
    Value(PyObjectRef),
    Missing,
    Unsafe,
}

impl InlineAttrResult {
    #[inline(always)]
    fn into_value(self) -> Option<PyObjectRef> {
        match self {
            Self::Value(value) => Some(value),
            Self::Missing | Self::Unsafe => None,
        }
    }
}

#[inline(always)]
fn unpack_local_name(instr: Instruction) -> Option<(usize, usize)> {
    if instr.op != Opcode::LoadFastLoadAttr {
        return None;
    }
    Some(((instr.arg >> 16) as usize, (instr.arg & 0xFFFF) as usize))
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
