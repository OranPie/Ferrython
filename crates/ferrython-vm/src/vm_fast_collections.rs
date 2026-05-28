//! Fast collection opcode helpers for the VM dispatch loop.

use crate::frame::Frame;
use compact_str::CompactString;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{
    FxHashKeyMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{BorrowedIntKey, BorrowedStrKey, HashableKey, PyInt};

pub(crate) enum FastCollectionResult {
    Handled,
    Fallback,
}

pub(crate) enum FastFusedCollectionResult {
    Handled,
    HandledChain,
    Fallback,
    UnboundLocal(usize),
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
pub(crate) fn try_fast_fused_collection(
    frame: &mut Frame,
    instr: Instruction,
) -> FastFusedCollectionResult {
    match instr.op {
        Opcode::LoadConstLoadFastContainsStoreFast => {
            try_const_fast_contains_store(frame, instr.arg)
        }
        Opcode::LoadFastLoadConstSubscrStoreFast => try_fast_const_subscr_store(frame, instr.arg),
        Opcode::LoadFastLoadFastSubscrStoreFast => try_fast_fast_subscr_store(frame, instr.arg),
        Opcode::LoadFastLoadFastLoadFastStoreSubscr => {
            try_fast_fast_fast_store_subscr(frame, instr.arg)
        }
        Opcode::LoadFastLoadFastContainsStoreFast => try_fast_fast_contains_store(frame, instr.arg),
        _ => FastFusedCollectionResult::Fallback,
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
fn local_ref(frame: &Frame, idx: usize) -> Option<&PyObjectRef> {
    unsafe { frame.locals.get_unchecked(idx).as_ref() }
}

#[inline(always)]
fn set_local(frame: &mut Frame, idx: usize, value: PyObjectRef) {
    unsafe { *frame.locals.get_unchecked_mut(idx) = Some(value) };
}

#[inline(always)]
fn store_bool_local(frame: &mut Frame, idx: usize, value: bool) {
    let dest = unsafe { frame.locals.get_unchecked_mut(idx) };
    if let Some(ref mut arc) = dest {
        if let Some(obj) = PyObjectRef::get_mut(arc) {
            obj.payload = PyObjectPayload::Bool(value);
            return;
        }
    }
    *dest = Some(PyObject::bool_val(value));
}

#[inline(always)]
fn sequence_index(len: usize, idx: i64) -> Option<usize> {
    let actual = if idx < 0 { idx + len as i64 } else { idx };
    if actual >= 0 && (actual as usize) < len {
        Some(actual as usize)
    } else {
        None
    }
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

#[inline(always)]
fn try_const_fast_contains_store(frame: &mut Frame, arg: u32) -> FastFusedCollectionResult {
    let not_in = (arg >> 31) != 0;
    let const_idx = ((arg >> 20) & 0x3FF) as usize;
    let fast_idx = ((arg >> 10) & 0x3FF) as usize;
    let store_idx = (arg & 0x3FF) as usize;
    let needle = unsafe { frame.constant_cache.get_unchecked(const_idx) };
    let Some(haystack) = local_ref(frame, fast_idx) else {
        return FastFusedCollectionResult::UnboundLocal(fast_idx);
    };
    let Some(is_in) = contains_known(needle, haystack) else {
        return FastFusedCollectionResult::Fallback;
    };
    store_bool_local(frame, store_idx, if not_in { !is_in } else { is_in });
    FastFusedCollectionResult::Handled
}

#[inline(always)]
fn try_fast_const_subscr_store(frame: &mut Frame, arg: u32) -> FastFusedCollectionResult {
    let fast_idx = ((arg >> 20) & 0x3FF) as usize;
    let const_idx = ((arg >> 10) & 0x3FF) as usize;
    let store_idx = (arg & 0x3FF) as usize;
    let Some(obj) = local_ref(frame, fast_idx) else {
        return FastFusedCollectionResult::UnboundLocal(fast_idx);
    };
    let key = unsafe { frame.constant_cache.get_unchecked(const_idx) };
    let Some(value) = subscr_known(obj, key) else {
        return FastFusedCollectionResult::Fallback;
    };
    set_local(frame, store_idx, value);
    FastFusedCollectionResult::Handled
}

#[inline(always)]
fn try_fast_fast_subscr_store(frame: &mut Frame, arg: u32) -> FastFusedCollectionResult {
    let container_idx = (arg >> 24) as usize;
    let key_idx = ((arg >> 16) & 0xFF) as usize;
    let store_idx = ((arg >> 8) & 0xFF) as usize;
    let Some(obj) = local_ref(frame, container_idx) else {
        return FastFusedCollectionResult::UnboundLocal(container_idx);
    };
    let Some(key) = local_ref(frame, key_idx) else {
        return FastFusedCollectionResult::UnboundLocal(key_idx);
    };
    let Some(value) = subscr_known(obj, key) else {
        return FastFusedCollectionResult::Fallback;
    };
    set_local(frame, store_idx, value);
    FastFusedCollectionResult::HandledChain
}

#[inline(always)]
fn try_fast_fast_fast_store_subscr(frame: &mut Frame, arg: u32) -> FastFusedCollectionResult {
    let val_idx = (arg >> 24) as usize;
    let container_idx = ((arg >> 16) & 0xFF) as usize;
    let key_idx = ((arg >> 8) & 0xFF) as usize;
    let Some(value) = local_ref(frame, val_idx) else {
        return FastFusedCollectionResult::UnboundLocal(val_idx);
    };
    let Some(obj) = local_ref(frame, container_idx) else {
        return FastFusedCollectionResult::UnboundLocal(container_idx);
    };
    let Some(key) = local_ref(frame, key_idx) else {
        return FastFusedCollectionResult::UnboundLocal(key_idx);
    };
    if store_subscr_known(obj, key, value) {
        FastFusedCollectionResult::Handled
    } else {
        FastFusedCollectionResult::Fallback
    }
}

#[inline(always)]
fn try_fast_fast_contains_store(frame: &mut Frame, arg: u32) -> FastFusedCollectionResult {
    let needle_idx = (arg >> 24) as usize;
    let haystack_idx = ((arg >> 16) & 0xFF) as usize;
    let store_idx = ((arg >> 8) & 0xFF) as usize;
    let negate = (arg & 1) != 0;
    let Some(needle) = local_ref(frame, needle_idx) else {
        return FastFusedCollectionResult::UnboundLocal(needle_idx);
    };
    let Some(haystack) = local_ref(frame, haystack_idx) else {
        return FastFusedCollectionResult::UnboundLocal(haystack_idx);
    };
    let Some(is_in) = contains_known(needle, haystack) else {
        return FastFusedCollectionResult::Fallback;
    };
    store_bool_local(frame, store_idx, if negate { !is_in } else { is_in });
    FastFusedCollectionResult::HandledChain
}

#[inline(always)]
fn subscr_known(obj: &PyObjectRef, key: &PyObjectRef) -> Option<PyObjectRef> {
    match (&obj.payload, &key.payload) {
        (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
            let items = unsafe { &*items_arc.data_ptr() };
            sequence_index(items.len(), *idx).map(|actual| items[actual].clone())
        }
        (PyObjectPayload::Tuple(items), PyObjectPayload::Int(PyInt::Small(idx))) => {
            sequence_index(items.len(), *idx).map(|actual| items[actual].clone())
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => unsafe { &*map.data_ptr() }
            .get(&BorrowedStrKey(s.as_str()))
            .cloned(),
        (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
            unsafe { &*map.data_ptr() }
                .get(&BorrowedIntKey(*n))
                .cloned()
        }
        _ => None,
    }
}

#[inline(always)]
fn store_subscr_known(obj: &PyObjectRef, key: &PyObjectRef, value: &PyObjectRef) -> bool {
    match (&obj.payload, &key.payload) {
        (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
            let hk = HashableKey::Int(PyInt::Small(*n));
            unsafe { &mut *map.data_ptr() }.insert(hk, value.clone());
            true
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
            let hk = HashableKey::str_key(s.to_compact_string());
            unsafe { &mut *map.data_ptr() }.insert(hk, value.clone());
            true
        }
        (PyObjectPayload::Dict(map), PyObjectPayload::Bool(b)) => {
            let hk = HashableKey::Int(PyInt::Small(*b as i64));
            unsafe { &mut *map.data_ptr() }.insert(hk, value.clone());
            true
        }
        (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
            let items = unsafe { &mut *items_arc.data_ptr() };
            if let Some(actual) = sequence_index(items.len(), *idx) {
                items[actual] = value.clone();
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

#[inline(always)]
fn contains_known(needle: &PyObjectRef, haystack: &PyObjectRef) -> Option<bool> {
    match &haystack.payload {
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
                _ => None,
            }
        }
        PyObjectPayload::List(items) => {
            let items = unsafe { &*items.data_ptr() };
            Some(items.iter().any(|item| item_matches(item, needle)))
        }
        PyObjectPayload::Tuple(items) => Some(items.iter().any(|item| item_matches(item, needle))),
        PyObjectPayload::Str(haystack_s) => {
            if let PyObjectPayload::Str(needle_s) = &needle.payload {
                Some(haystack_s.contains(needle_s.as_str()))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[inline(always)]
fn item_matches(item: &PyObjectRef, needle: &PyObjectRef) -> bool {
    match (&item.payload, &needle.payload) {
        (PyObjectPayload::Int(PyInt::Small(a)), PyObjectPayload::Int(PyInt::Small(b))) => a == b,
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a == b,
        _ => item.is_same(needle),
    }
}
