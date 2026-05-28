//! Fast call helpers for direct `CallFunction` dispatch paths.

use crate::frame::{BlockKind, Frame, FramePool, ScopeKind, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::CodeFlags;
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use std::rc::Rc;

pub(crate) enum FastCallResult {
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

    let mut new_frame = Frame::new_from_pool(
        Rc::clone(&pf.code),
        pf.globals.clone(),
        builtins.clone(),
        Rc::clone(&pf.constant_cache),
        frame_pool,
    );
    new_frame.scope_kind = ScopeKind::Function;
    let args_start = func_idx + 1;
    unsafe {
        let base = frame.stack.as_ptr();
        new_frame.locals[0] = Some(std::ptr::read(base.add(func_idx)));
        for i in 0..arg_count {
            new_frame.locals[i + 1] = Some(std::ptr::read(base.add(args_start + i)));
        }
        frame.stack.set_len(func_idx);
    }
    FastCallResult::NewFrame(new_frame)
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
    let msg: CompactString = if arg_count >= 1 {
        match &unsafe { frame.stack.get_unchecked(func_idx + 1) }.payload {
            PyObjectPayload::Str(s) => s.to_compact_string(),
            _ => CompactString::from(
                unsafe { frame.stack.get_unchecked(func_idx + 1) }.py_to_string(),
            ),
        }
    } else {
        CompactString::default()
    };
    let args: Vec<PyObjectRef> = if arg_count > 0 {
        (0..arg_count)
            .map(|i| unsafe { frame.stack.get_unchecked(func_idx + 1 + i) }.clone())
            .collect()
    } else {
        Vec::new()
    };
    unsafe {
        frame.stack.set_len(func_idx);
    }
    let inst = PyObject::exception_instance_with_args(kind, msg, args.clone());
    if matches!(
        kind,
        ExceptionKind::ExceptionGroup | ExceptionKind::BaseExceptionGroup
    ) {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut attrs = ei.ensure_attrs().write();
            if !args.is_empty() {
                attrs.insert(CompactString::from("message"), args[0].clone());
            }
            if args.len() >= 2 {
                let exc_list = match &args[1].payload {
                    PyObjectPayload::List(_) => args[1].clone(),
                    PyObjectPayload::Tuple(items) => PyObject::list((**items).clone()),
                    _ => PyObject::list(vec![args[1].clone()]),
                };
                attrs.insert(CompactString::from("exceptions"), exc_list);
            }
        }
        if args.len() >= 2 {
            crate::vm_call::attach_eg_methods_pub(&inst);
        }
    }
    push_stack(frame, inst);
    FastCallResult::Pushed
}
