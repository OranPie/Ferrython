//! Fast collection opcode helpers for the VM dispatch loop.

use crate::frame::Frame;
use compact_str::CompactString;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{FxHashKeyMap, PyObject, PyObjectPayload};
use ferrython_core::types::{BorrowedIntKey, BorrowedStrKey, HashableKey, PyInt};

pub(crate) enum FastCollectionResult {
    Handled,
    Fallback,
}

#[inline(always)]
pub(crate) fn try_fast_collection(frame: &mut Frame, instr: Instruction) -> FastCollectionResult {
    match instr.op {
        Opcode::BinarySubscr => try_binary_subscr(frame),
        Opcode::StoreSubscr => try_store_subscr(frame),
        Opcode::ListAppend => {
            fast_list_append(frame, instr.arg as usize);
            FastCollectionResult::Handled
        }
        Opcode::MapAdd => try_map_add(frame, instr.arg as usize),
        Opcode::SetAdd => try_set_add(frame, instr.arg as usize),
        _ => FastCollectionResult::Fallback,
    }
}

#[inline(always)]
fn stack_ref(frame: &Frame, idx: usize) -> &ferrython_core::object::PyObjectRef {
    unsafe { frame.stack.get_unchecked(idx) }
}

#[inline(always)]
fn pop_stack(frame: &mut Frame) -> ferrython_core::object::PyObjectRef {
    frame.stack.pop().expect("stack underflow")
}

#[inline(always)]
fn store_binary_result(frame: &mut Frame, value: ferrython_core::object::PyObjectRef) {
    unsafe { frame.binary_op_result(value) };
}

#[inline(always)]
fn try_binary_subscr(frame: &mut Frame) -> FastCollectionResult {
    let len = frame.stack.len();
    let obj = stack_ref(frame, len - 2);
    let key = stack_ref(frame, len - 1);
    match (&obj.payload, &key.payload) {
        (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
            let items = unsafe { &*items_arc.data_ptr() };
            let i = *idx;
            let actual = if i < 0 { i + items.len() as i64 } else { i };
            if actual >= 0 && (actual as usize) < items.len() {
                store_binary_result(frame, items[actual as usize].clone());
                FastCollectionResult::Handled
            } else {
                FastCollectionResult::Fallback
            }
        }
        (PyObjectPayload::Tuple(items), PyObjectPayload::Int(PyInt::Small(idx))) => {
            let i = *idx;
            let actual = if i < 0 { i + items.len() as i64 } else { i };
            if actual >= 0 && (actual as usize) < items.len() {
                store_binary_result(frame, items[actual as usize].clone());
                FastCollectionResult::Handled
            } else {
                FastCollectionResult::Fallback
            }
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
            let val = unsafe { &*map.data_ptr() }
                .get(&BorrowedStrKey(s.as_str()))
                .cloned();
            if let Some(v) = val {
                store_binary_result(frame, v);
                FastCollectionResult::Handled
            } else {
                FastCollectionResult::Fallback
            }
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
            let val = unsafe { &*map.data_ptr() }
                .get(&BorrowedIntKey(*n))
                .cloned();
            if let Some(v) = val {
                store_binary_result(frame, v);
                FastCollectionResult::Handled
            } else {
                FastCollectionResult::Fallback
            }
        }
        (PyObjectPayload::Str(s), PyObjectPayload::Int(PyInt::Small(idx))) => {
            let chars: Vec<char> = s.chars().collect();
            let i = *idx;
            let actual = if i < 0 { i + chars.len() as i64 } else { i };
            if actual >= 0 && (actual as usize) < chars.len() {
                let ch = chars[actual as usize];
                store_binary_result(
                    frame,
                    PyObject::str_val(CompactString::from(ch.to_string())),
                );
                FastCollectionResult::Handled
            } else {
                FastCollectionResult::Fallback
            }
        }
        _ => FastCollectionResult::Fallback,
    }
}

#[inline(always)]
fn try_store_subscr(frame: &mut Frame) -> FastCollectionResult {
    let len = frame.stack.len();
    let key = stack_ref(frame, len - 1);
    let obj = stack_ref(frame, len - 2);
    match (&obj.payload, &key.payload) {
        (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
            let items = unsafe { &mut *items_arc.data_ptr() };
            let i = *idx;
            let actual = if i < 0 { i + items.len() as i64 } else { i };
            if actual >= 0 && (actual as usize) < items.len() {
                let v = unsafe { std::ptr::read(frame.stack.as_ptr().add(len - 3)) };
                items[actual as usize] = v;
                unsafe {
                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 1));
                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 2));
                    frame.stack.set_len(len - 3);
                }
                FastCollectionResult::Handled
            } else {
                FastCollectionResult::Fallback
            }
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
            let hk = HashableKey::str_key(s.to_compact_string());
            insert_dict_stack_value(frame, len, map.data_ptr(), hk);
            FastCollectionResult::Handled
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
            insert_dict_stack_value(
                frame,
                len,
                map.data_ptr(),
                HashableKey::Int(PyInt::Small(*n)),
            );
            FastCollectionResult::Handled
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Bool(b)) => {
            insert_dict_stack_value(
                frame,
                len,
                map.data_ptr(),
                HashableKey::Int(PyInt::Small(*b as i64)),
            );
            FastCollectionResult::Handled
        }
        _ => FastCollectionResult::Fallback,
    }
}

#[inline(always)]
fn insert_dict_stack_value(
    frame: &mut Frame,
    len: usize,
    map_ptr: *mut FxHashKeyMap,
    key: HashableKey,
) {
    unsafe {
        let v = std::ptr::read(frame.stack.as_ptr().add(len - 3));
        (&mut *map_ptr).insert(key, v);
        std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 1));
        std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 2));
        frame.stack.set_len(len - 3);
    }
}

#[inline(always)]
fn fast_list_append(frame: &mut Frame, idx: usize) {
    let item = pop_stack(frame);
    let stack_pos = frame.stack.len() - idx;
    if let PyObjectPayload::List(items) = &stack_ref(frame, stack_pos).payload {
        unsafe { &mut *items.data_ptr() }.push(item);
    }
}

#[inline(always)]
fn simple_hash_key(obj: &ferrython_core::object::PyObjectRef) -> Option<HashableKey> {
    match &obj.payload {
        PyObjectPayload::Int(PyInt::Small(n)) => Some(HashableKey::Int(PyInt::Small(*n))),
        PyObjectPayload::Str(s) => Some(HashableKey::str_key(s.to_compact_string())),
        PyObjectPayload::Bool(b) => Some(HashableKey::Int(PyInt::Small(*b as i64))),
        _ => None,
    }
}

#[inline(always)]
fn try_map_add(frame: &mut Frame, idx: usize) -> FastCollectionResult {
    let len = frame.stack.len();
    let key_ref = stack_ref(frame, len - 2);
    let stack_pos = len - 2 - idx;
    let Some(hk) = simple_hash_key(key_ref) else {
        return FastCollectionResult::Fallback;
    };
    let map_ptr = if let PyObjectPayload::Dict(m) = &stack_ref(frame, stack_pos).payload {
        Some(m.data_ptr())
    } else {
        None
    };
    if let Some(map_ptr) = map_ptr {
        let value = pop_stack(frame);
        let _key = pop_stack(frame);
        let map = unsafe { &mut *map_ptr };
        if map.capacity() == 0 {
            map.reserve(32);
        }
        map.insert(hk, value);
        FastCollectionResult::Handled
    } else {
        FastCollectionResult::Fallback
    }
}

#[inline(always)]
fn try_set_add(frame: &mut Frame, idx: usize) -> FastCollectionResult {
    let len = frame.stack.len();
    let item_ref = stack_ref(frame, len - 1);
    let stack_pos = len - 1 - idx;
    let Some(hk) = simple_hash_key(item_ref) else {
        return FastCollectionResult::Fallback;
    };
    let set_ptr = if let PyObjectPayload::Set(s) = &stack_ref(frame, stack_pos).payload {
        Some(s.data_ptr())
    } else {
        None
    };
    if let Some(set_ptr) = set_ptr {
        let item = pop_stack(frame);
        let set = unsafe { &mut *set_ptr };
        if set.capacity() == 0 {
            set.reserve(16);
        }
        set.entry(hk).or_insert(item);
        FastCollectionResult::Handled
    } else {
        FastCollectionResult::Fallback
    }
}
