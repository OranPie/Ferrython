//! Fast call helpers for direct `CallFunction` dispatch paths.

use crate::frame::{BlockKind, Frame, FramePool, ScopeKind, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::CodeFlags;
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyFunction;
use std::rc::Rc;

pub(crate) enum FastCallResult {
    Pushed,
    NewFrame(Frame),
    Fallback,
}

pub(crate) enum FastGlobalFunctionResult {
    Pushed,
    NewFrame(Frame),
    Fallback,
}

pub(crate) enum FastExceptionFlowResult {
    Error(PyException),
    PoppedFinallyNone,
    Fallback,
}

pub(crate) enum FastDerefResult {
    Loaded,
    Stored,
    Fallback,
}

pub(crate) enum FastBlockResult {
    Handled,
    PopExcept,
    Fallback,
}

#[inline(always)]
fn push_stack(frame: &mut Frame, value: PyObjectRef) {
    unsafe {
        let stack = &mut frame.stack;
        if stack.len() < stack.capacity() {
            let len = stack.len();
            std::ptr::write(stack.as_mut_ptr().add(len), value);
            stack.set_len(len + 1);
        } else {
            stack.push(value);
        }
    }
}

#[inline(always)]
pub(crate) fn try_fast_raise_varargs_one(frame: &mut Frame) -> FastExceptionFlowResult {
    let Some(tos) = frame.stack.last() else {
        return FastExceptionFlowResult::Fallback;
    };
    match &tos.payload {
        PyObjectPayload::ExceptionInstance(ei) => {
            let kind = ei.kind;
            let msg = ei.message.clone();
            let original = frame.stack.pop().expect("stack underflow");
            FastExceptionFlowResult::Error(PyException::with_original(kind, msg, original))
        }
        PyObjectPayload::ExceptionType(kind) => {
            let kind = *kind;
            let _ = frame.stack.pop();
            FastExceptionFlowResult::Error(PyException::new(kind, ""))
        }
        _ => FastExceptionFlowResult::Fallback,
    }
}

#[inline(always)]
pub(crate) fn try_fast_end_finally_none(frame: &mut Frame) -> FastExceptionFlowResult {
    if frame.pending_return.is_some() || frame.pending_jump.is_some() {
        return FastExceptionFlowResult::Fallback;
    }
    let Some(tos) = frame.stack.last() else {
        return FastExceptionFlowResult::Fallback;
    };
    if !matches!(&tos.payload, PyObjectPayload::None) {
        return FastExceptionFlowResult::Fallback;
    }
    let _ = frame.stack.pop();
    FastExceptionFlowResult::PoppedFinallyNone
}

#[inline(always)]
pub(crate) fn try_fast_load_deref(frame: &mut Frame, idx: usize) -> FastDerefResult {
    let val = unsafe { &*frame.cells[idx].data_ptr() };
    if let Some(value) = val {
        push_stack(frame, value.clone());
        FastDerefResult::Loaded
    } else {
        FastDerefResult::Fallback
    }
}

#[inline(always)]
pub(crate) fn fast_store_deref(frame: &mut Frame, idx: usize) -> FastDerefResult {
    let value = frame.stack.pop().expect("stack underflow");
    unsafe { *frame.cells[idx].data_ptr() = Some(value) };
    FastDerefResult::Stored
}

#[inline(always)]
pub(crate) fn try_fast_block_control(frame: &mut Frame, instr: Instruction) -> FastBlockResult {
    match instr.op {
        Opcode::SetupExcept => {
            frame.push_block(BlockKind::Except, instr.arg as usize);
            FastBlockResult::Handled
        }
        Opcode::SetupFinally => {
            frame.push_block(BlockKind::Finally, instr.arg as usize);
            FastBlockResult::Handled
        }
        Opcode::PopBlock => {
            frame.pop_block();
            FastBlockResult::Handled
        }
        Opcode::PopExcept => {
            frame.pop_block();
            FastBlockResult::PopExcept
        }
        _ => FastBlockResult::Fallback,
    }
}

#[inline(always)]
fn global_function_call_kind(pf: &PyFunction, arg_count: usize, frame: &Frame) -> u8 {
    if pf.has_code_override() {
        return 0;
    }
    if pf.is_simple && pf.code.arg_count as usize == arg_count {
        if is_trivial_const_return(pf) {
            3
        } else if Rc::ptr_eq(&pf.code, &frame.code) {
            2
        } else {
            1
        }
    } else {
        0
    }
}

#[inline(always)]
fn is_trivial_const_return(pf: &PyFunction) -> bool {
    (pf.code.instructions.len() == 2
        && pf.code.instructions[0].op == Opcode::LoadConst
        && pf.code.instructions[1].op == Opcode::ReturnValue)
        || (pf.code.instructions.len() == 1
            && pf.code.instructions[0].op == Opcode::LoadConstReturnValue)
}

#[inline(always)]
pub(crate) fn try_fast_global_function_call(
    frame: &mut Frame,
    func_obj: &PyObjectRef,
    builtins: &SharedBuiltins,
    frame_pool: &mut FramePool,
    arg_count: usize,
    trace_active_now: bool,
    inline_simple_args: impl Fn(&PyFunction, &[PyObjectRef]) -> Option<PyObjectRef>,
    inline_recursive_base: impl Fn(
        &[Instruction],
        &[PyObjectRef],
        &[PyObjectRef],
    ) -> Option<PyObjectRef>,
) -> FastGlobalFunctionResult {
    let PyObjectPayload::Function(pf) = &func_obj.payload else {
        return FastGlobalFunctionResult::Fallback;
    };
    let call_kind = global_function_call_kind(pf, arg_count, frame);
    if call_kind == 0 {
        return FastGlobalFunctionResult::Fallback;
    }

    if call_kind == 3 && !trace_active_now {
        let ret_val = pf.constant_cache[pf.code.instructions[0].arg as usize].clone();
        let args_start = frame.stack.len() - arg_count;
        unsafe {
            let base = frame.stack.as_ptr();
            for i in 0..arg_count {
                let _ = std::ptr::read(base.add(args_start + i));
            }
            frame.stack.set_len(args_start);
        }
        push_stack(frame, ret_val);
        return FastGlobalFunctionResult::Pushed;
    }

    let args_start = frame.stack.len() - arg_count;
    let args: Vec<PyObjectRef> = frame.stack[args_start..args_start + arg_count]
        .iter()
        .cloned()
        .collect();
    let mini_result = match call_kind {
        1 if arg_count > 0 => inline_simple_args(pf, &args),
        2 => inline_recursive_base(&frame.code.instructions, &frame.constant_cache, &args),
        _ => None,
    };
    if let Some(ret_val) = mini_result.filter(|_| !trace_active_now) {
        frame.stack.truncate(args_start);
        push_stack(frame, ret_val);
        return FastGlobalFunctionResult::Pushed;
    }

    let mut new_frame = if call_kind == 2 {
        unsafe { Frame::new_recursive(frame, frame_pool) }
    } else if call_kind == 1 {
        let func_clone = func_obj.clone();
        unsafe {
            let pf_ptr = match &func_clone.payload {
                PyObjectPayload::Function(pf) => &**pf as *const PyFunction,
                _ => std::hint::unreachable_unchecked(),
            };
            Frame::new_borrowed(&*pf_ptr, func_clone, builtins, frame_pool)
        }
    } else {
        let mut f = Frame::new_from_pool(
            Rc::clone(&pf.code),
            pf.globals.clone(),
            builtins.clone(),
            Rc::clone(&pf.constant_cache),
            frame_pool,
        );
        f.scope_kind = ScopeKind::Function;
        f
    };
    unsafe {
        let base = frame.stack.as_ptr();
        for i in 0..arg_count {
            new_frame.locals[i] = Some(std::ptr::read(base.add(args_start + i)));
        }
        frame.stack.set_len(args_start);
    }
    FastGlobalFunctionResult::NewFrame(new_frame)
}

#[inline(always)]
pub(crate) fn try_fast_instance_call(
    frame: &mut Frame,
    builtins: &SharedBuiltins,
    frame_pool: &mut FramePool,
    func_idx: usize,
    arg_count: usize,
) -> FastCallResult {
    let func_obj = unsafe { frame.stack.get_unchecked(func_idx) };
    let PyObjectPayload::Instance(inst) = &func_obj.payload else {
        return FastCallResult::Fallback;
    };
    let call_method = if let PyObjectPayload::Class(cd) = &inst.class.payload {
        let vt = unsafe { &*cd.method_vtable.data_ptr() };
        if !vt.is_empty() {
            vt.get("__call__").cloned()
        } else {
            unsafe { &*cd.namespace.data_ptr() }
                .get("__call__")
                .cloned()
        }
    } else {
        None
    };
    let Some(call_method) = call_method else {
        return FastCallResult::Fallback;
    };
    let PyObjectPayload::Function(pf) = &call_method.payload else {
        return FastCallResult::Fallback;
    };
    if pf.code.arg_count as usize != arg_count + 1
        || pf.code.kwonlyarg_count != 0
        || pf.code.flags.contains(CodeFlags::VARARGS)
        || pf.code.flags.contains(CodeFlags::VARKEYWORDS)
        || pf.code.flags.contains(CodeFlags::GENERATOR)
        || pf.code.flags.contains(CodeFlags::COROUTINE)
    {
        return FastCallResult::Fallback;
    }

    let mut new_frame = if pf.closure.is_empty() {
        let mut frame = Frame::new_from_pool(
            Rc::clone(&pf.code),
            pf.globals.clone(),
            builtins.clone(),
            Rc::clone(&pf.constant_cache),
            frame_pool,
        );
        frame.scope_kind = ScopeKind::Function;
        frame
    } else {
        Frame::new_closure_from_pool(
            Rc::clone(&pf.code),
            pf.globals.clone(),
            builtins.clone(),
            Rc::clone(&pf.constant_cache),
            &pf.closure,
            frame_pool,
        )
    };
    let args_start = func_idx + 1;
    unsafe {
        let base = frame.stack.as_ptr();
        new_frame.locals[0] = Some(std::ptr::read(base.add(func_idx)));
        for i in 0..arg_count {
            new_frame.locals[i + 1] = Some(std::ptr::read(base.add(args_start + i)));
        }
        frame.stack.set_len(func_idx);
    }
    link_cellvars_from_locals(&mut new_frame);
    FastCallResult::NewFrame(new_frame)
}

#[inline(always)]
fn link_cellvars_from_locals(frame: &mut Frame) {
    if frame.code.cellvars.is_empty() {
        return;
    }
    for (cell_idx, cell_name) in frame.code.cellvars.iter().enumerate() {
        for (var_idx, var_name) in frame.code.varnames.iter().enumerate() {
            if cell_name == var_name {
                if let Some(val) = frame.locals[var_idx].take() {
                    unsafe {
                        *frame.cells[cell_idx].data_ptr() = Some(val);
                    }
                }
                break;
            }
        }
    }
}

#[inline(always)]
pub(crate) fn try_fast_simple_class_call(
    frame: &mut Frame,
    builtins: &SharedBuiltins,
    frame_pool: &mut FramePool,
    func_idx: usize,
    arg_count: usize,
) -> FastCallResult {
    let cls_obj = unsafe { frame.stack.get_unchecked(func_idx) };
    let PyObjectPayload::Class(cd) = &cls_obj.payload else {
        return FastCallResult::Fallback;
    };
    if !cd.is_simple_class.get()
        || cd.namespace.read().contains_key("__new__")
        || cd.is_dict_subclass
        || cd.builtin_base_name.is_some()
        || class_inherits_type(cd)
        || ferrython_core::object::is_property_subclass_class(cls_obj)
    {
        return FastCallResult::Fallback;
    }

    let vt = unsafe { &*cd.method_vtable.data_ptr() };
    let init_fn = if !vt.is_empty() {
        vt.get("__init__").cloned()
    } else {
        None
    }
    .or_else(|| {
        cd.namespace
            .read()
            .get("__init__")
            .cloned()
            .or_else(|| ferrython_core::object::lookup_in_class_mro(cls_obj, "__init__"))
    });

    if let Some(init_fn) = init_fn {
        let PyObjectPayload::Function(pf) = &init_fn.payload else {
            return FastCallResult::Fallback;
        };
        if !pf.is_simple || pf.code.arg_count as usize != arg_count + 1 {
            return FastCallResult::Fallback;
        }

        let instance = PyObject::instance(cls_obj.clone());
        let mut new_frame = Frame::new_from_pool(
            Rc::clone(&pf.code),
            pf.globals.clone(),
            builtins.clone(),
            Rc::clone(&pf.constant_cache),
            frame_pool,
        );
        new_frame.scope_kind = ScopeKind::Function;
        new_frame.locals[0] = Some(instance.clone());
        let args_start = func_idx + 1;
        unsafe {
            let base = frame.stack.as_ptr();
            for i in 0..arg_count {
                new_frame.locals[1 + i] = Some(std::ptr::read(base.add(args_start + i)));
            }
            let _func = std::ptr::read(base.add(func_idx));
            frame.stack.set_len(func_idx);
        }
        push_stack(frame, instance);
        new_frame.discard_return = true;
        return FastCallResult::NewFrame(new_frame);
    }

    if arg_count > 0 && !cd.is_exception_subclass {
        return FastCallResult::Fallback;
    }

    let instance = PyObject::instance(cls_obj.clone());
    if cd.is_exception_subclass {
        if let PyObjectPayload::Instance(inst) = &instance.payload {
            let mut args_vec = Vec::with_capacity(arg_count);
            for i in 0..arg_count {
                args_vec.push(unsafe { frame.stack.get_unchecked(func_idx + 1 + i) }.clone());
            }
            let mut attrs = inst.attrs.write();
            if arg_count == 1 {
                attrs.insert(CompactString::from("message"), args_vec[0].clone());
            }
            attrs.insert(CompactString::from("args"), PyObject::tuple(args_vec));
        }
    }
    unsafe {
        let base = frame.stack.as_ptr();
        for i in 0..=arg_count {
            let _ = std::ptr::read(base.add(func_idx + i));
        }
        frame.stack.set_len(func_idx);
    }
    push_stack(frame, instance);
    FastCallResult::Pushed
}

fn class_inherits_type(cd: &ferrython_core::object::ClassData) -> bool {
    cd.bases
        .iter()
        .chain(cd.mro.iter())
        .any(|base| match &base.payload {
            PyObjectPayload::BuiltinType(name) => name.as_str() == "type",
            PyObjectPayload::Class(base_cd) => {
                base_cd.name.as_str() == "type" || class_inherits_type(base_cd)
            }
            _ => false,
        })
}

#[inline(always)]
pub(crate) fn try_fast_exception_type_call(
    frame: &mut Frame,
    func_idx: usize,
    arg_count: usize,
) -> FastCallResult {
    let func_obj = unsafe { frame.stack.get_unchecked(func_idx) };
    let PyObjectPayload::ExceptionType(kind) = &func_obj.payload else {
        return FastCallResult::Fallback;
    };
    let kind = *kind;
    let args: Vec<PyObjectRef> = if arg_count > 0 {
        (0..arg_count)
            .map(|i| unsafe { frame.stack.get_unchecked(func_idx + 1 + i) }.clone())
            .collect()
    } else {
        Vec::new()
    };
    let inst = match crate::vm_call::build_builtin_exception_instance(kind, args, &[]) {
        Ok(inst) => inst,
        Err(_) => return FastCallResult::Fallback,
    };
    unsafe {
        let base = frame.stack.as_ptr();
        for i in 0..=arg_count {
            let _ = std::ptr::read(base.add(func_idx + i));
        }
        frame.stack.set_len(func_idx);
    }
    push_stack(frame, inst);
    FastCallResult::Pushed
}
