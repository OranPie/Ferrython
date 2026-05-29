//! Fast method-call helpers for direct method dispatch paths.

use crate::frame::{Frame, FramePool, ScopeKind, SharedBuiltins};
use crate::vm::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    FxHashKeyMap, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{BorrowedIntKey, BorrowedStrKey, HashableKey, PyInt};
use std::rc::Rc;

use crate::vm_method_cache::{is_interned_append, is_interned_pop};

pub(crate) enum FastMethodResult {
    Handled(PyObjectRef),
    HandledNone,
    Fallback,
    Error(PyException),
}

pub(crate) enum FastMethodPopTopResult {
    Handled,
    HandledChain,
    Fallback,
    Error(PyException),
}

#[inline(always)]
pub(crate) fn try_fast_python_method_frame(
    frame: &mut Frame,
    builtins: &SharedBuiltins,
    frame_pool: &mut FramePool,
    arg_count: usize,
) -> Option<Frame> {
    let stack_len = frame.stack.len();
    let base_idx = stack_len.checked_sub(arg_count + 2)?;
    let is_simple_method = match &unsafe { frame.stack.get_unchecked(base_idx) }.payload {
        PyObjectPayload::Function(pf) => {
            pf.is_simple && pf.code.arg_count as usize == arg_count + 1
        }
        PyObjectPayload::None => false,
        _ => false,
    };
    if !is_simple_method {
        return None;
    }

    let method_idx = base_idx;
    let arg_start = stack_len - arg_count;
    let mut new_frame = unsafe {
        let method_obj: PyObjectRef = std::ptr::read(frame.stack.as_ptr().add(method_idx));
        let pf_ptr = match &method_obj.payload {
            PyObjectPayload::Function(pf) => &**pf as *const ferrython_core::types::PyFunction,
            _ => std::hint::unreachable_unchecked(),
        };
        Frame::new_borrowed(&*pf_ptr, method_obj, builtins, frame_pool)
    };

    unsafe {
        let base = frame.stack.as_ptr();
        for i in 0..arg_count {
            new_frame.locals[i + 1] = Some(std::ptr::read(base.add(arg_start + i)));
        }
        new_frame.locals[0] = Some(std::ptr::read(base.add(arg_start - 1)));
        frame.stack.set_len(method_idx);
    }

    if Rc::ptr_eq(&frame.code, &new_frame.code) {
        if let Some(ref cache) = frame.global_cache {
            new_frame.global_cache = Some(cache.clone());
            new_frame.global_cache_version = frame.global_cache_version;
        }
    }
    new_frame.scope_kind = ScopeKind::Function;
    Some(new_frame)
}

impl VirtualMachine {
    pub(crate) fn call_builtin_method_fallback(
        &mut self,
        frame: &mut Frame,
        arg_count: usize,
    ) -> PyResult<PyObjectRef> {
        if arg_count == 1 {
            let a0 = pop_stack(frame);
            let receiver = pop_stack(frame);
            let name_obj = pop_stack(frame);
            let PyObjectPayload::Str(ref name) = name_obj.payload else {
                return Ok(PyObject::none());
            };
            return self.call_builtin_method_fallback_1(receiver, name.as_str(), a0);
        }

        if arg_count == 0 {
            let receiver = pop_stack(frame);
            let name_obj = pop_stack(frame);
            let PyObjectPayload::Str(ref name) = name_obj.payload else {
                return Ok(PyObject::none());
            };
            return self.call_builtin_method_fallback_0(receiver, name.as_str());
        }

        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(pop_stack(frame));
        }
        args.reverse();
        let receiver = pop_stack(frame);
        let name_obj = pop_stack(frame);
        let PyObjectPayload::Str(ref name) = name_obj.payload else {
            return Ok(PyObject::none());
        };
        crate::builtins::call_method(&receiver, name.as_str(), &args)
    }

    fn call_builtin_method_fallback_0(
        &mut self,
        receiver: PyObjectRef,
        name: &str,
    ) -> PyResult<PyObjectRef> {
        if name == "sort" && matches!(&receiver.payload, PyObjectPayload::List(_)) {
            let mut values = if let PyObjectPayload::List(items) = &receiver.payload {
                items.read().clone()
            } else {
                Vec::new()
            };
            self.vm_sort(&mut values).and_then(|()| {
                if let PyObjectPayload::List(items) = &receiver.payload {
                    *items.write() = values;
                }
                Ok(PyObject::none())
            })
        } else {
            crate::builtins::call_method(&receiver, name, &[])
        }
    }

    fn call_builtin_method_fallback_1(
        &mut self,
        receiver: PyObjectRef,
        name: &str,
        a0: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        if name == "join"
            && matches!(
                &receiver.payload,
                PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
            )
            && is_vm_collectable_join_arg(&a0)
        {
            return self.collect_iterable(&a0).and_then(|items| {
                crate::builtins::call_method(&receiver, name, &[PyObject::list(items)])
            });
        }

        if is_set_collect_method(name)
            && matches!(
                &receiver.payload,
                PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
            )
            && is_vm_collectable_set_arg(&a0)
        {
            return self.collect_iterable(&a0).and_then(|items| {
                crate::builtins::call_method(&receiver, name, &[PyObject::list(items)])
            });
        }

        if name == "extend" && matches!(&receiver.payload, PyObjectPayload::List(_)) {
            if list_extend_needs_vm_collect(&a0) {
                return self.collect_iterable(&a0).and_then(|items| {
                    crate::builtins::call_method(&receiver, "extend", &[PyObject::list(items)])
                });
            }
            return crate::builtins::call_method(&receiver, name, &[a0]);
        }

        crate::builtins::call_method(&receiver, name, &[a0])
    }
}

#[inline(always)]
pub(crate) fn try_fast_builtin_method(frame: &mut Frame, arg_count: usize) -> FastMethodResult {
    let stack_len = frame.stack.len();
    let Some(base_idx) = stack_len.checked_sub(arg_count + 2) else {
        return FastMethodResult::Fallback;
    };
    let name = match &unsafe { frame.stack.get_unchecked(base_idx) }.payload {
        PyObjectPayload::Str(name) => name.clone(),
        _ => return FastMethodResult::Fallback,
    };

    match arg_count {
        0 => try_builtin_method_0(frame, base_idx, name.as_str()),
        1 => try_builtin_method_1(frame, base_idx, name.as_str()),
        2 => try_builtin_method_2(frame, base_idx, name.as_str()),
        _ => FastMethodResult::Fallback,
    }
}

#[inline(always)]
pub(crate) fn try_fast_builtin_method_poptop(
    frame: &mut Frame,
    arg_count: usize,
) -> FastMethodPopTopResult {
    let stack_len = frame.stack.len();
    let Some(base_idx) = stack_len.checked_sub(arg_count + 2) else {
        return FastMethodPopTopResult::Fallback;
    };

    if arg_count == 1
        && is_interned_append(unsafe { frame.stack.get_unchecked(base_idx) })
        && matches!(
            &unsafe { frame.stack.get_unchecked(base_idx + 1) }.payload,
            PyObjectPayload::List(_)
        )
    {
        append_list_arg(frame);
        return FastMethodPopTopResult::HandledChain;
    }

    if arg_count == 0
        && is_interned_pop(unsafe { frame.stack.get_unchecked(base_idx) })
        && matches!(
            &unsafe { frame.stack.get_unchecked(base_idx + 1) }.payload,
            PyObjectPayload::List(_)
        )
    {
        return match pop_list_receiver(frame) {
            Ok(()) => FastMethodPopTopResult::HandledChain,
            Err(err) => FastMethodPopTopResult::Error(err),
        };
    }

    let name = match &unsafe { frame.stack.get_unchecked(base_idx) }.payload {
        PyObjectPayload::Str(name) => name.clone(),
        _ => return FastMethodPopTopResult::Fallback,
    };

    match arg_count {
        0 => try_builtin_method_poptop_0(frame, base_idx, name.as_str()),
        1 => try_builtin_method_poptop_1(frame, base_idx, name.as_str()),
        2 => match try_builtin_method_2(frame, base_idx, name.as_str()) {
            FastMethodResult::Handled(_) | FastMethodResult::HandledNone => {
                FastMethodPopTopResult::Handled
            }
            FastMethodResult::Fallback => FastMethodPopTopResult::Fallback,
            FastMethodResult::Error(err) => FastMethodPopTopResult::Error(err),
        },
        _ => FastMethodPopTopResult::Fallback,
    }
}

#[inline(always)]
fn try_builtin_method_poptop_0(
    frame: &mut Frame,
    base_idx: usize,
    name: &str,
) -> FastMethodPopTopResult {
    match (
        name,
        &unsafe { frame.stack.get_unchecked(base_idx + 1) }.payload,
    ) {
        ("pop", PyObjectPayload::List(_)) => match pop_list_receiver(frame) {
            Ok(()) => FastMethodPopTopResult::HandledChain,
            Err(err) => FastMethodPopTopResult::Error(err),
        },
        ("sort", PyObjectPayload::List(_)) => FastMethodPopTopResult::Fallback,
        _ => match try_builtin_method_0(frame, base_idx, name) {
            FastMethodResult::Handled(_) | FastMethodResult::HandledNone => {
                FastMethodPopTopResult::Handled
            }
            FastMethodResult::Fallback => FastMethodPopTopResult::Fallback,
            FastMethodResult::Error(err) => FastMethodPopTopResult::Error(err),
        },
    }
}

#[inline(always)]
fn try_builtin_method_poptop_1(
    frame: &mut Frame,
    base_idx: usize,
    name: &str,
) -> FastMethodPopTopResult {
    match (
        name,
        &unsafe { frame.stack.get_unchecked(base_idx + 1) }.payload,
    ) {
        ("append", PyObjectPayload::List(_)) => {
            append_list_arg(frame);
            FastMethodPopTopResult::HandledChain
        }
        _ => match try_builtin_method_1(frame, base_idx, name) {
            FastMethodResult::Handled(_) => FastMethodPopTopResult::Handled,
            FastMethodResult::HandledNone => FastMethodPopTopResult::HandledChain,
            FastMethodResult::Fallback => FastMethodPopTopResult::Fallback,
            FastMethodResult::Error(err) => FastMethodPopTopResult::Error(err),
        },
    }
}

#[inline(always)]
fn try_builtin_method_0(frame: &mut Frame, base_idx: usize, name: &str) -> FastMethodResult {
    match (
        name,
        &unsafe { frame.stack.get_unchecked(base_idx + 1) }.payload,
    ) {
        ("strip", PyObjectPayload::Str(_))
        | ("lstrip", PyObjectPayload::Str(_))
        | ("rstrip", PyObjectPayload::Str(_))
        | ("lower", PyObjectPayload::Str(_))
        | ("upper", PyObjectPayload::Str(_)) => {
            let receiver = pop_stack(frame);
            drop(pop_stack(frame));
            let PyObjectPayload::Str(s) = &receiver.payload else {
                unreachable!()
            };
            let result = match name {
                "strip" => PyObject::str_val(CompactString::from(s.trim())),
                "lstrip" => PyObject::str_val(CompactString::from(s.trim_start())),
                "rstrip" => PyObject::str_val(CompactString::from(s.trim_end())),
                "lower" => PyObject::str_val(CompactString::from(s.to_lowercase())),
                _ => PyObject::str_val(CompactString::from(s.to_uppercase())),
            };
            FastMethodResult::Handled(result)
        }
        ("sort", PyObjectPayload::List(_)) => FastMethodResult::Fallback,
        _ => {
            let receiver = pop_stack(frame);
            drop(pop_stack(frame));
            call_direct_method(&receiver, name, &[])
        }
    }
}

#[inline(always)]
fn try_builtin_method_1(frame: &mut Frame, base_idx: usize, name: &str) -> FastMethodResult {
    if name == "join"
        || name == "extend"
        || matches!(
            name,
            "union"
                | "intersection"
                | "difference"
                | "symmetric_difference"
                | "update"
                | "intersection_update"
                | "difference_update"
                | "symmetric_difference_update"
                | "issubset"
                | "issuperset"
                | "isdisjoint"
                | "__or__"
                | "__and__"
                | "__sub__"
                | "__xor__"
        )
    {
        return FastMethodResult::Fallback;
    }

    match (
        name,
        &unsafe { frame.stack.get_unchecked(base_idx + 1) }.payload,
    ) {
        ("append", PyObjectPayload::List(_)) => {
            let len = frame.stack.len();
            unsafe {
                let value = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                let receiver = &*frame.stack.as_ptr().add(len - 2);
                let PyObjectPayload::List(items) = &receiver.payload else {
                    unreachable!()
                };
                (&mut *items.data_ptr()).push(value);
                let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                let _name = std::ptr::read(frame.stack.as_ptr().add(len - 3));
                frame.stack.set_len(len - 3);
            }
            FastMethodResult::HandledNone
        }
        ("get", PyObjectPayload::Dict(_)) => {
            let key = pop_stack(frame);
            let receiver = pop_stack(frame);
            drop(pop_stack(frame));
            let PyObjectPayload::Dict(map) = &receiver.payload else {
                unreachable!()
            };
            match dict_get(map, &key) {
                Ok(value) => FastMethodResult::Handled(value),
                Err(err) => FastMethodResult::Error(err),
            }
        }
        ("add", PyObjectPayload::Set(_)) => {
            let item = pop_stack(frame);
            let receiver = pop_stack(frame);
            drop(pop_stack(frame));
            let PyObjectPayload::Set(set) = &receiver.payload else {
                unreachable!()
            };
            if let Some(key) = simple_hashable_key(&item) {
                unsafe { &mut *set.data_ptr() }.entry(key).or_insert(item);
                FastMethodResult::HandledNone
            } else {
                call_direct_method(&receiver, "add", &[item])
            }
        }
        ("startswith", PyObjectPayload::Str(_)) | ("endswith", PyObjectPayload::Str(_)) => {
            let arg = pop_stack(frame);
            let receiver = pop_stack(frame);
            drop(pop_stack(frame));
            if let (PyObjectPayload::Str(s), PyObjectPayload::Str(prefix)) =
                (&receiver.payload, &arg.payload)
            {
                let matched = if name == "startswith" {
                    s.starts_with(prefix.as_str())
                } else {
                    s.ends_with(prefix.as_str())
                };
                FastMethodResult::Handled(PyObject::bool_val(matched))
            } else {
                call_direct_method(&receiver, name, &[arg])
            }
        }
        _ => {
            let arg = pop_stack(frame);
            let receiver = pop_stack(frame);
            drop(pop_stack(frame));
            call_direct_method(&receiver, name, &[arg])
        }
    }
}

#[inline(always)]
fn try_builtin_method_2(frame: &mut Frame, _base_idx: usize, name: &str) -> FastMethodResult {
    let a1 = pop_stack(frame);
    let a0 = pop_stack(frame);
    let receiver = pop_stack(frame);
    drop(pop_stack(frame));
    call_direct_method(&receiver, name, &[a0, a1])
}

#[inline(always)]
fn pop_stack(frame: &mut Frame) -> PyObjectRef {
    frame.stack.pop().expect("stack underflow")
}

#[inline(always)]
fn append_list_arg(frame: &mut Frame) {
    let len = frame.stack.len();
    unsafe {
        let value = std::ptr::read(frame.stack.as_ptr().add(len - 1));
        let receiver = &*frame.stack.as_ptr().add(len - 2);
        let PyObjectPayload::List(items) = &receiver.payload else {
            unreachable!()
        };
        (&mut *items.data_ptr()).push(value);
        let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 2));
        let _name = std::ptr::read(frame.stack.as_ptr().add(len - 3));
        frame.stack.set_len(len - 3);
    }
}

#[inline(always)]
fn pop_list_receiver(frame: &mut Frame) -> PyResult<()> {
    let len = frame.stack.len();
    unsafe {
        let receiver = &*frame.stack.as_ptr().add(len - 1);
        let PyObjectPayload::List(items) = &receiver.payload else {
            unreachable!()
        };
        if (&mut *items.data_ptr()).pop().is_some() {
            let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 1));
            let _name = std::ptr::read(frame.stack.as_ptr().add(len - 2));
            frame.stack.set_len(len - 2);
            Ok(())
        } else {
            Err(PyException::index_error("pop from empty list"))
        }
    }
}

#[inline(always)]
fn dict_get(
    map: &ferrython_core::object::PyCell<FxHashKeyMap>,
    key: &PyObjectRef,
) -> PyResult<PyObjectRef> {
    let entries = unsafe { &*map.data_ptr() };
    Ok(match &key.payload {
        PyObjectPayload::Str(s) => entries.get(&BorrowedStrKey(s.as_str())).cloned(),
        PyObjectPayload::Int(PyInt::Small(n)) => entries.get(&BorrowedIntKey(*n)).cloned(),
        PyObjectPayload::Bool(b) => entries.get(&BorrowedIntKey(*b as i64)).cloned(),
        _ => entries.get(&key.to_hashable_key()?).cloned(),
    }
    .unwrap_or_else(PyObject::none))
}

#[inline(always)]
fn simple_hashable_key(obj: &PyObjectRef) -> Option<HashableKey> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Some(HashableKey::str_key(s.to_compact_string())),
        PyObjectPayload::Int(i) => Some(HashableKey::Int(i.clone())),
        PyObjectPayload::Bool(b) => Some(HashableKey::Bool(*b)),
        _ => None,
    }
}

#[inline(always)]
fn is_vm_collectable_join_arg(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Generator(_)
            | PyObjectPayload::Instance(_)
            | PyObjectPayload::Iterator(_)
            | PyObjectPayload::VecIter(_)
            | PyObjectPayload::WeakValueIter(_)
            | PyObjectPayload::WeakKeyIter(_)
            | PyObjectPayload::DequeIter(_)
            | PyObjectPayload::RefIter { .. }
            | PyObjectPayload::RevRefIter { .. }
    )
}

#[inline(always)]
fn is_vm_collectable_set_arg(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_) | PyObjectPayload::Iterator(_)
    )
}

#[inline(always)]
fn list_extend_needs_vm_collect(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_) => true,
        PyObjectPayload::Iterator(iter_data) => matches!(
            &*iter_data.read(),
            IteratorData::Enumerate { .. }
                | IteratorData::Zip { .. }
                | IteratorData::MapOne { .. }
                | IteratorData::Map { .. }
                | IteratorData::Filter { .. }
                | IteratorData::FilterFalse { .. }
                | IteratorData::Sentinel { .. }
        ),
        _ => false,
    }
}

#[inline(always)]
fn is_set_collect_method(name: &str) -> bool {
    matches!(
        name,
        "union"
            | "intersection"
            | "difference"
            | "symmetric_difference"
            | "update"
            | "intersection_update"
            | "difference_update"
            | "symmetric_difference_update"
            | "issubset"
            | "issuperset"
            | "isdisjoint"
            | "__or__"
            | "__and__"
            | "__sub__"
            | "__xor__"
    )
}

#[inline(always)]
fn call_direct_method(
    receiver: &PyObjectRef,
    name: &str,
    args: &[PyObjectRef],
) -> FastMethodResult {
    let result = if name.as_bytes().first() == Some(&b'_') {
        crate::builtins::call_method(receiver, name, args)
    } else {
        match &receiver.payload {
            PyObjectPayload::Str(s) => crate::builtins::call_str_method(s.as_str(), name, args),
            PyObjectPayload::List(items) => {
                crate::builtins::call_list_method(receiver, items, name, args)
            }
            PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => {
                crate::builtins::call_dict_method(map, name, args, Some(receiver.clone()))
            }
            PyObjectPayload::Set(map) => crate::builtins::call_set_method(map, name, args),
            PyObjectPayload::Tuple(items) => crate::builtins::call_tuple_method(items, name, args),
            _ => crate::builtins::call_method(receiver, name, args),
        }
    };
    match result {
        Ok(value) => FastMethodResult::Handled(value),
        Err(err) => FastMethodResult::Error(err),
    }
}
