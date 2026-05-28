//! Fast iterator setup and `ForIterStoreFast` helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::error::PyException;
use ferrython_core::object::{
    is_hidden_dict_key, GeneratorState, IteratorData, PyCell, PyObject, PyObjectPayload,
    PyObjectRef, SyncUsize,
};
use ferrython_core::types::{HashableKey, PyInt};
use std::rc::Rc;

pub(crate) enum FastGetIterResult {
    Handled,
    Fallback,
}

pub(crate) enum FastForIterStoreResult {
    HandledChain,
    Generator(Rc<PyCell<GeneratorState>>),
    Fallback,
}

pub(crate) enum FastForIterResult {
    Handled,
    HandledChain,
    Generator(Rc<PyCell<GeneratorState>>),
    Fallback,
    Error(PyException),
}

#[inline(always)]
pub(crate) fn try_fast_get_iter(frame: &mut Frame) -> FastGetIterResult {
    let obj = unsafe { frame.stack.get_unchecked(frame.stack.len() - 1) };
    match &obj.payload {
        PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. }
        | PyObjectPayload::Generator(_) => FastGetIterResult::Handled,
        PyObjectPayload::List(_)
        | PyObjectPayload::Tuple(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::MappingProxy(_)
        | PyObjectPayload::DictKeys { .. }
        | PyObjectPayload::DictValues { .. }
        | PyObjectPayload::DictItems { .. } => {
            let iter = PyObject::wrap(PyObjectPayload::RefIter {
                source: obj.clone(),
                index: SyncUsize::new(0),
            });
            replace_stack_top(frame, iter);
            FastGetIterResult::Handled
        }
        _ => FastGetIterResult::Fallback,
    }
}

#[inline(always)]
pub(crate) fn try_fast_for_iter(
    frame: &mut Frame,
    jump_target: usize,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastForIterResult {
    let iter = unsafe { frame.peek_unchecked().clone() };
    match &iter.payload {
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 {
                cur >= ri.stop
            } else {
                cur <= ri.stop
            };
            if done {
                drop_stack_top(frame);
                frame.ip = jump_target;
            } else {
                ri.current.set(cur + ri.step);
                push(frame, PyObject::int(cur));
            }
            FastForIterResult::Handled
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                let value = data.items[idx].clone();
                data.index.set(idx + 1);
                push(frame, value);
            } else {
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterResult::Handled
        }
        PyObjectPayload::RefIter { source, index } => {
            try_for_ref_iter(frame, jump_target, source, index, instr_base, instr_count)
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            if idx == usize::MAX || idx == 0 {
                index.set(usize::MAX);
                drop_stack_top(frame);
                frame.ip = jump_target;
            } else if let PyObjectPayload::List(cell) = &source.payload {
                let pos = idx - 1;
                let items = unsafe { &*cell.data_ptr() };
                if pos < items.len() {
                    let value = items[pos].clone();
                    index.set(pos);
                    push(frame, value);
                } else {
                    index.set(usize::MAX);
                    drop_stack_top(frame);
                    frame.ip = jump_target;
                }
            } else {
                index.set(usize::MAX);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterResult::Handled
        }
        PyObjectPayload::Iterator(iter_data) => {
            try_for_iterator_data(frame, jump_target, iter_data, instr_base, instr_count)
        }
        PyObjectPayload::Generator(gen_arc) => FastForIterResult::Generator(gen_arc.clone()),
        _ => FastForIterResult::Fallback,
    }
}

#[inline(always)]
pub(crate) fn try_fast_for_iter_store(
    frame: &mut Frame,
    jump_target: usize,
    store_idx: usize,
) -> FastForIterStoreResult {
    let iter = unsafe { frame.peek_unchecked().clone() };
    match &iter.payload {
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 {
                cur >= ri.stop
            } else {
                cur <= ri.stop
            };
            if done {
                drop_stack_top(frame);
                frame.ip = jump_target;
            } else {
                ri.current.set(cur + ri.step);
                let dest_slot = unsafe { frame.locals.get_unchecked_mut(store_idx) };
                if let Some(ref mut arc) = dest_slot {
                    if let Some(obj) = PyObjectRef::get_mut(arc) {
                        obj.payload = PyObjectPayload::Int(PyInt::Small(cur));
                        return FastForIterStoreResult::HandledChain;
                    }
                }
                *dest_slot = Some(PyObject::wrap_leaf(PyObjectPayload::Int(PyInt::Small(cur))));
            }
            FastForIterStoreResult::HandledChain
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                let obj = data.items[idx].clone();
                data.index.set(idx + 1);
                set_local(frame, store_idx, obj);
            } else {
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterStoreResult::HandledChain
        }
        PyObjectPayload::RefIter { source, index } => {
            if index.get() == usize::MAX {
                drop_stack_top(frame);
                frame.ip = jump_target;
                return FastForIterStoreResult::HandledChain;
            }
            let idx = index.get();
            let item = ref_iter_item(source, idx);
            if let Some(value) = item {
                index.set(idx + 1);
                set_local(frame, store_idx, value);
            } else {
                index.set(usize::MAX);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterStoreResult::HandledChain
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            if idx == usize::MAX || idx == 0 {
                index.set(usize::MAX);
                drop_stack_top(frame);
                frame.ip = jump_target;
            } else if let PyObjectPayload::List(cell) = &source.payload {
                let pos = idx - 1;
                let items = unsafe { &*cell.data_ptr() };
                if pos < items.len() {
                    let obj = items[pos].clone();
                    index.set(pos);
                    set_local(frame, store_idx, obj);
                } else {
                    index.set(usize::MAX);
                    drop_stack_top(frame);
                    frame.ip = jump_target;
                }
            } else {
                index.set(usize::MAX);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterStoreResult::HandledChain
        }
        PyObjectPayload::Iterator(iter_data) => {
            try_store_from_iterator_data(frame, iter_data, jump_target, store_idx)
        }
        PyObjectPayload::Generator(gen_arc) => FastForIterStoreResult::Generator(gen_arc.clone()),
        _ => FastForIterStoreResult::Fallback,
    }
}

#[inline(always)]
fn try_store_from_iterator_data(
    frame: &mut Frame,
    iter_data: &PyCell<IteratorData>,
    jump_target: usize,
    store_idx: usize,
) -> FastForIterStoreResult {
    let mut data = iter_data.write();
    match &mut *data {
        IteratorData::Range {
            current,
            stop,
            step,
        } => {
            let done = if *step > 0 {
                *current >= *stop
            } else {
                *current <= *stop
            };
            if done {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            } else {
                let value = PyObject::int(*current);
                *current += *step;
                drop(data);
                set_local(frame, store_idx, value);
            }
            FastForIterStoreResult::HandledChain
        }
        IteratorData::List { items, index } => {
            if *index < items.len() {
                let value = items[*index].clone();
                *index += 1;
                drop(data);
                set_local(frame, store_idx, value);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterStoreResult::HandledChain
        }
        IteratorData::Tuple { items, index } => {
            if *index < items.len() {
                let value = items[*index].clone();
                *index += 1;
                drop(data);
                set_local(frame, store_idx, value);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterStoreResult::HandledChain
        }
        IteratorData::DictKeys { keys, index } => {
            if *index < keys.len() {
                let value = keys[*index].clone();
                *index += 1;
                drop(data);
                set_local(frame, store_idx, value);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterStoreResult::HandledChain
        }
        _ => FastForIterStoreResult::Fallback,
    }
}

#[inline(always)]
fn try_for_iterator_data(
    frame: &mut Frame,
    jump_target: usize,
    iter_data: &PyCell<IteratorData>,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastForIterResult {
    let mut data = iter_data.write();
    match &mut *data {
        IteratorData::Range {
            current,
            stop,
            step,
        } => {
            let done = if *step > 0 {
                *current >= *stop
            } else {
                *current <= *stop
            };
            if done {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            } else {
                let value = PyObject::int(*current);
                *current += *step;
                drop(data);
                push(frame, value);
            }
            FastForIterResult::Handled
        }
        IteratorData::List { items, index } => {
            if *index < items.len() {
                let value = items[*index].clone();
                *index += 1;
                drop(data);
                push(frame, value);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterResult::Handled
        }
        IteratorData::Tuple { items, index } => {
            if *index < items.len() {
                let value = items[*index].clone();
                *index += 1;
                drop(data);
                push(frame, value);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterResult::Handled
        }
        IteratorData::Enumerate {
            source,
            index,
            cached_tuple: _,
        } => {
            let idx = *index;
            let advanced = advance_enumerate_source(source);
            match advanced {
                Some(Some(value)) => {
                    *index = idx + 1;
                    let idx_obj = PyObject::int(idx);
                    drop(data);
                    push_pair_or_unpack(frame, idx_obj, value, instr_base, instr_count)
                }
                Some(None) => {
                    drop(data);
                    drop_stack_top(frame);
                    frame.ip = jump_target;
                    FastForIterResult::Handled
                }
                None => {
                    drop(data);
                    FastForIterResult::Fallback
                }
            }
        }
        IteratorData::Zip {
            sources,
            strict,
            cached_tuple,
            items_buf,
        } => {
            let is_strict = *strict;
            let n = sources.len();
            if n == 2 {
                let (first, second) = advance_zip_pair(sources);
                match (first, second) {
                    (Some(Some(a)), Some(Some(b))) => {
                        drop(data);
                        push_pair_or_unpack(frame, a, b, instr_base, instr_count)
                    }
                    (Some(None), Some(None)) if is_strict => {
                        drop(data);
                        drop_stack_top(frame);
                        frame.ip = jump_target;
                        FastForIterResult::Handled
                    }
                    (Some(None), _) | (_, Some(None)) if is_strict => {
                        drop(data);
                        FastForIterResult::Error(PyException::value_error(
                            "zip() has arguments with different lengths",
                        ))
                    }
                    (Some(None), _) | (_, Some(None)) => {
                        drop(data);
                        drop_stack_top(frame);
                        frame.ip = jump_target;
                        FastForIterResult::Handled
                    }
                    _ => {
                        drop(data);
                        FastForIterResult::Fallback
                    }
                }
            } else {
                items_buf.clear();
                let mut all_ok = true;
                let mut exhausted_count = 0usize;
                let mut needs_vm = false;
                for src in sources.iter() {
                    match advance_source_inline(src) {
                        Some(Some(value)) => items_buf.push(value),
                        Some(None) => {
                            exhausted_count += 1;
                            if is_strict {
                                items_buf.push(PyObject::none());
                            } else {
                                all_ok = false;
                                break;
                            }
                        }
                        None => {
                            needs_vm = true;
                            break;
                        }
                    }
                }
                if needs_vm {
                    items_buf.clear();
                    drop(data);
                    FastForIterResult::Fallback
                } else if !all_ok || (is_strict && exhausted_count > 0 && exhausted_count == n) {
                    if is_strict && exhausted_count > 0 && exhausted_count != n {
                        items_buf.clear();
                        drop(data);
                        FastForIterResult::Error(PyException::value_error(
                            "zip() has arguments with different lengths",
                        ))
                    } else {
                        items_buf.clear();
                        drop(data);
                        drop_stack_top(frame);
                        frame.ip = jump_target;
                        FastForIterResult::Handled
                    }
                } else {
                    let tuple = reuse_or_create_tuple(items_buf, cached_tuple, n);
                    drop(data);
                    push(frame, tuple);
                    FastForIterResult::Handled
                }
            }
        }
        IteratorData::DictEntries {
            source,
            owner: _,
            index,
            cached_tuple,
        } => {
            let map = unsafe { &*source.data_ptr() };
            while *index < map.len() {
                let (key, _) = map.get_index(*index).unwrap();
                if !is_hidden_dict_key(key) {
                    break;
                }
                *index += 1;
            }
            if *index < map.len() {
                let (key, value) = map.get_index(*index).unwrap();
                let key = key.to_object();
                let value = value.clone();
                *index += 1;
                let tuple = reuse_dict_entry_tuple(cached_tuple, key, value);
                drop(data);
                push(frame, tuple);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterResult::Handled
        }
        IteratorData::DictKeys { keys, index } => {
            if *index < keys.len() {
                let value = keys[*index].clone();
                *index += 1;
                drop(data);
                push(frame, value);
            } else {
                drop(data);
                drop_stack_top(frame);
                frame.ip = jump_target;
            }
            FastForIterResult::Handled
        }
        _ => {
            drop(data);
            FastForIterResult::Fallback
        }
    }
}

#[inline(always)]
fn try_for_ref_iter(
    frame: &mut Frame,
    jump_target: usize,
    source: &PyObjectRef,
    index: &SyncUsize,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastForIterResult {
    if index.get() == usize::MAX {
        drop_stack_top(frame);
        frame.ip = jump_target;
        return FastForIterResult::Handled;
    }
    let idx = index.get();
    if let PyObjectPayload::DictItems { map: cell, .. } = &source.payload {
        let map = unsafe { &*cell.data_ptr() };
        if idx < map.len() {
            let (key, value) = map.get_index(idx).unwrap();
            let key = key.to_object();
            let value = value.clone();
            index.set(idx + 1);
            return push_pair_or_unpack(frame, key, value, instr_base, instr_count);
        }
        index.set(usize::MAX);
        drop_stack_top(frame);
        frame.ip = jump_target;
        return FastForIterResult::Handled;
    }
    if let Some(value) = ref_iter_item(source, idx) {
        index.set(idx + 1);
        push(frame, value);
    } else {
        index.set(usize::MAX);
        drop_stack_top(frame);
        frame.ip = jump_target;
    }
    FastForIterResult::Handled
}

#[inline(always)]
fn push_pair_or_unpack(
    frame: &mut Frame,
    first: PyObjectRef,
    second: PyObjectRef,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastForIterResult {
    let next_ip = frame.ip;
    if next_ip + 2 < instr_count {
        let unpack = unsafe { *instr_base.add(next_ip) };
        if unpack.op == Opcode::UnpackSequence && unpack.arg == 2 {
            let first_store = unsafe { *instr_base.add(next_ip + 1) };
            let second_store = unsafe { *instr_base.add(next_ip + 2) };
            if first_store.op == Opcode::StoreFast {
                if second_store.op == Opcode::StoreFast {
                    set_local(frame, first_store.arg as usize, first);
                    set_local(frame, second_store.arg as usize, second);
                    frame.ip = next_ip + 3;
                    return FastForIterResult::Handled;
                }
                if second_store.op == Opcode::StoreFastJumpAbsolute {
                    set_local(frame, first_store.arg as usize, first);
                    set_local(frame, (second_store.arg >> 16) as usize, second);
                    frame.ip = (second_store.arg & 0xFFFF) as usize;
                    return FastForIterResult::HandledChain;
                }
            }
            push(frame, second);
            push(frame, first);
            frame.ip = next_ip + 1;
            return FastForIterResult::Handled;
        }
    }
    push(frame, PyObject::tuple(vec![first, second]));
    FastForIterResult::Handled
}

#[inline(always)]
fn ref_iter_item(source: &PyObjectRef, idx: usize) -> Option<PyObjectRef> {
    match &source.payload {
        PyObjectPayload::List(cell) => {
            let items = unsafe { &*cell.data_ptr() };
            items.get(idx).cloned()
        }
        PyObjectPayload::Tuple(items) => items.get(idx).cloned(),
        PyObjectPayload::Dict(cell)
        | PyObjectPayload::MappingProxy(cell)
        | PyObjectPayload::DictKeys { map: cell, .. } => {
            let map = unsafe { &*cell.data_ptr() };
            if idx < map.len() {
                let (hk, _) = map.get_index(idx).unwrap();
                Some(match hk {
                    HashableKey::Int(PyInt::Small(n)) => PyObject::int(*n),
                    HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
                    _ => hk.to_object(),
                })
            } else {
                None
            }
        }
        PyObjectPayload::DictValues { map: cell, .. } => {
            let map = unsafe { &*cell.data_ptr() };
            map.get_index(idx).map(|(_, value)| value.clone())
        }
        PyObjectPayload::DictItems { map: cell, .. } => {
            let map = unsafe { &*cell.data_ptr() };
            map.get_index(idx)
                .map(|(key, value)| PyObject::tuple(vec![key.to_object(), value.clone()]))
        }
        _ => None,
    }
}

#[inline(always)]
fn advance_enumerate_source(source: &PyObjectRef) -> Option<Option<PyObjectRef>> {
    if let PyObjectPayload::RefIter {
        source: src,
        index: src_idx,
    } = &source.payload
    {
        let idx = src_idx.get();
        match &src.payload {
            PyObjectPayload::List(cell) => {
                let items = unsafe { &*cell.data_ptr() };
                if idx < items.len() {
                    src_idx.set(idx + 1);
                    return Some(Some(items[idx].clone()));
                }
                return Some(None);
            }
            PyObjectPayload::Tuple(items) => {
                if idx < items.len() {
                    src_idx.set(idx + 1);
                    return Some(Some(items[idx].clone()));
                }
                return Some(None);
            }
            _ => {}
        }
    }
    if let PyObjectPayload::VecIter(data) = &source.payload {
        let idx = data.index.get();
        if idx < data.items.len() {
            data.index.set(idx + 1);
            return Some(Some(data.items[idx].clone()));
        }
        return Some(None);
    }
    advance_source_inline(source)
}

#[inline(always)]
fn advance_zip_pair(
    sources: &[PyObjectRef],
) -> (Option<Option<PyObjectRef>>, Option<Option<PyObjectRef>>) {
    if let (
        PyObjectPayload::RefIter {
            source: src0,
            index: idx0,
        },
        PyObjectPayload::RefIter {
            source: src1,
            index: idx1,
        },
    ) = (&sources[0].payload, &sources[1].payload)
    {
        if let (PyObjectPayload::List(cell0), PyObjectPayload::List(cell1)) =
            (&src0.payload, &src1.payload)
        {
            let items0 = unsafe { &*cell0.data_ptr() };
            let items1 = unsafe { &*cell1.data_ptr() };
            let i0 = idx0.get();
            let i1 = idx1.get();
            if i0 < items0.len() && i1 < items1.len() {
                idx0.set(i0 + 1);
                idx1.set(i1 + 1);
                return (
                    Some(Some(items0[i0].clone())),
                    Some(Some(items1[i1].clone())),
                );
            }
            let first = if i0 >= items0.len() {
                Some(None)
            } else {
                Some(Some(items0[i0].clone()))
            };
            let second = if i1 >= items1.len() {
                Some(None)
            } else {
                Some(Some(items1[i1].clone()))
            };
            if first.as_ref().is_some_and(|value| value.is_some()) {
                idx0.set(i0 + 1);
            }
            if second.as_ref().is_some_and(|value| value.is_some()) {
                idx1.set(i1 + 1);
            }
            return (first, second);
        }
    }
    (
        advance_source_inline(&sources[0]),
        advance_source_inline(&sources[1]),
    )
}

#[inline(always)]
fn advance_source_inline(source: &PyObjectRef) -> Option<Option<PyObjectRef>> {
    match &source.payload {
        PyObjectPayload::Iterator(data) => {
            let mut data = data.write();
            match &mut *data {
                IteratorData::List { items, index } => {
                    if *index < items.len() {
                        let value = items[*index].clone();
                        *index += 1;
                        Some(Some(value))
                    } else {
                        Some(None)
                    }
                }
                IteratorData::Tuple { items, index } => {
                    if *index < items.len() {
                        let value = items[*index].clone();
                        *index += 1;
                        Some(Some(value))
                    } else {
                        Some(None)
                    }
                }
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    let done = if *step > 0 {
                        *current >= *stop
                    } else {
                        *current <= *stop
                    };
                    if done {
                        Some(None)
                    } else {
                        let value = PyObject::int(*current);
                        *current += *step;
                        Some(Some(value))
                    }
                }
                _ => None,
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 {
                cur >= ri.stop
            } else {
                cur <= ri.stop
            };
            if done {
                Some(None)
            } else {
                ri.current.set(cur + ri.step);
                Some(Some(PyObject::int(cur)))
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                data.index.set(idx + 1);
                Some(Some(data.items[idx].clone()))
            } else {
                Some(None)
            }
        }
        PyObjectPayload::RefIter { source, index } => {
            if index.get() == usize::MAX {
                return Some(None);
            }
            let idx = index.get();
            if let Some(value) = ref_iter_item(source, idx) {
                index.set(idx + 1);
                Some(Some(value))
            } else {
                index.set(usize::MAX);
                Some(None)
            }
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            if idx == usize::MAX || idx == 0 {
                index.set(usize::MAX);
                return Some(None);
            }
            if let PyObjectPayload::List(cell) = &source.payload {
                let pos = idx - 1;
                let items = unsafe { &*cell.data_ptr() };
                if pos < items.len() {
                    index.set(pos);
                    return Some(Some(items[pos].clone()));
                }
                index.set(usize::MAX);
                return Some(None);
            }
            None
        }
        _ => None,
    }
}

#[inline(always)]
fn reuse_or_create_tuple(
    items_buf: &mut Vec<PyObjectRef>,
    cached_tuple: &mut Option<PyObjectRef>,
    expected_len: usize,
) -> PyObjectRef {
    if items_buf.len() == expected_len {
        if let Some(cached) = cached_tuple {
            if let Some(obj) = PyObjectRef::get_mut(cached) {
                if let PyObjectPayload::Tuple(items) = &mut obj.payload {
                    if items.len() == expected_len {
                        for (idx, value) in items_buf.drain(..).enumerate() {
                            items[idx] = value;
                        }
                        return cached.clone();
                    }
                }
            }
            let values: Vec<_> = items_buf.drain(..).collect();
            let tuple = PyObject::tuple(values);
            *cached = tuple.clone();
            return tuple;
        }
        let values: Vec<_> = items_buf.drain(..).collect();
        let tuple = PyObject::tuple(values);
        *cached_tuple = Some(tuple.clone());
        return tuple;
    }
    let values: Vec<_> = items_buf.drain(..).collect();
    PyObject::tuple(values)
}

#[inline(always)]
fn reuse_dict_entry_tuple(
    cached_tuple: &mut Option<PyObjectRef>,
    key: PyObjectRef,
    value: PyObjectRef,
) -> PyObjectRef {
    if let Some(cached) = cached_tuple {
        if PyObjectRef::strong_count(cached) == 1 {
            unsafe {
                let obj_ptr = PyObjectRef::as_ptr(cached) as *mut PyObject;
                if let PyObjectPayload::Tuple(items) = &mut (*obj_ptr).payload {
                    items[0] = key;
                    items[1] = value;
                    return cached.clone();
                }
            }
        }
        let tuple = PyObject::tuple(vec![key, value]);
        *cached = tuple.clone();
        return tuple;
    }
    let tuple = PyObject::tuple(vec![key, value]);
    *cached_tuple = Some(tuple.clone());
    tuple
}

#[inline(always)]
fn replace_stack_top(frame: &mut Frame, value: PyObjectRef) {
    let len = frame.stack.len();
    unsafe { *frame.stack.get_unchecked_mut(len - 1) = value };
}

#[inline(always)]
fn drop_stack_top(frame: &mut Frame) {
    drop(frame.stack.pop().expect("stack underflow"));
}

#[inline(always)]
fn push(frame: &mut Frame, value: PyObjectRef) {
    unsafe { frame.push_unchecked(value) };
}

#[inline(always)]
fn set_local(frame: &mut Frame, idx: usize, value: PyObjectRef) {
    unsafe { *frame.locals.get_unchecked_mut(idx) = Some(value) };
}
