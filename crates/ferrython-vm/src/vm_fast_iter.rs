//! Fast iterator setup and `ForIterStoreFast` helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_core::object::{
    GeneratorState, IteratorData, PyCell, PyObject, PyObjectPayload, PyObjectRef, SyncUsize,
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
fn replace_stack_top(frame: &mut Frame, value: PyObjectRef) {
    let len = frame.stack.len();
    unsafe { *frame.stack.get_unchecked_mut(len - 1) = value };
}

#[inline(always)]
fn drop_stack_top(frame: &mut Frame) {
    drop(frame.stack.pop().expect("stack underflow"));
}

#[inline(always)]
fn set_local(frame: &mut Frame, idx: usize, value: PyObjectRef) {
    unsafe { *frame.locals.get_unchecked_mut(idx) = Some(value) };
}
