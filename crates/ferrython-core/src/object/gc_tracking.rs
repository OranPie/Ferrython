//! Cycle collector support for Python objects that can participate in reference cycles.

use crate::types::HashableKey;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::payload::*;
use super::ClassData;

// ── GC Tracking for cycle-capable objects (Instance, Dict, List) ──
// Static UnsafeCell: no TLS overhead — single-threaded GIL interpreter.
struct TrackedHolder(std::cell::UnsafeCell<Vec<PyWeakRef>>);
unsafe impl Sync for TrackedHolder {}
static TRACKED_OBJECTS: TrackedHolder = TrackedHolder(std::cell::UnsafeCell::new(Vec::new()));

/// Register the cycle collector callback with the GC crate.
pub fn init_gc() {
    ferrython_gc::register_cycle_collector(run_cycle_collection);
}

/// Cycle collection: find objects that are only reachable through
/// other tracked objects (i.e., they form a reference cycle).
/// Covers Instance, Dict, and List objects.
///
/// Algorithm (trial deletion, simplified for Arc):
/// 1. Purge dead weak refs from TRACKED_OBJECTS
/// 2. For each live tracked object, count PyObjectRef::strong_count()
/// 3. Count how many references each tracked object receives from other tracked objects
/// 4. If strong_count == internal_refs, the object is only reachable from within cycles
/// 5. Clear contents on unreachable objects to break cycles (dropping internal refs)
fn run_cycle_collection() -> usize {
    unsafe {
        let tracked = &mut *TRACKED_OBJECTS.0.get();

        // 1. Upgrade weak refs, purge dead ones, then include tracked intermediates.
        let mut roots: Vec<PyObjectRef> = tracked.iter().filter_map(|w| w.upgrade()).collect();
        tracked.retain(|w| w.strong_count() > 0);
        let mut alive = Vec::new();
        let mut seen = HashSet::new();
        for obj in roots.drain(..) {
            collect_gc_reachable(obj, &mut alive, &mut seen);
        }
        drop(roots);

        if alive.is_empty() {
            return 0;
        }

        // 2. Build pointer → index map for fast lookup
        let ptr_map: HashMap<usize, usize> = alive
            .iter()
            .enumerate()
            .map(|(i, obj)| (PyObjectRef::as_ptr(obj) as usize, i))
            .collect();

        // 3. Count internal references (refs from one tracked object to another)
        let mut internal_refs = vec![0usize; alive.len()];
        for obj in &alive {
            count_internal_refs(&obj.payload, &ptr_map, &mut internal_refs);
        }

        // 4. Trial deletion: objects where strong_count == internal_refs + 1
        // (+1 for our own `alive` Vec holding a ref)
        let mut garbage_indices: Vec<usize> = Vec::new();
        for (i, obj) in alive.iter().enumerate() {
            let strong = PyObjectRef::strong_count(obj);
            if strong <= internal_refs[i] + 1 {
                garbage_indices.push(i);
            }
        }

        // 5. Refine trial deletion against the candidate set itself. The
        // first pass counted references from every tracked object, including
        // live containers that merely hold a dead-looking object. Recompute
        // incoming references from candidates only; otherwise a live list that
        // points at an instance can make that instance look cyclic.
        loop {
            let candidate_set: HashSet<usize> = garbage_indices.iter().copied().collect();
            let mut candidate_internal_refs = vec![0usize; alive.len()];
            for &gi in &garbage_indices {
                count_internal_refs(&alive[gi].payload, &ptr_map, &mut candidate_internal_refs);
            }
            let refined: Vec<usize> = garbage_indices
                .iter()
                .copied()
                .filter(|&gi| {
                    let strong = PyObjectRef::strong_count(&alive[gi]);
                    strong <= candidate_internal_refs[gi] + 1
                })
                .collect();
            if refined.len() == candidate_set.len() {
                break;
            }
            garbage_indices = refined;
        }

        // 6. Verify: all garbage objects must only reference other garbage objects
        // (conservative: only collect fully isolated cycles)
        let candidate_ptrs: HashSet<usize> = garbage_indices
            .iter()
            .map(|&gi| PyObjectRef::as_ptr(&alive[gi]) as usize)
            .collect();
        let garbage_set: HashSet<usize> = garbage_indices.iter().copied().collect();
        let mut confirmed_garbage: Vec<usize> = Vec::new();
        for &gi in &garbage_indices {
            let obj = &alive[gi];
            if verify_all_refs_in_garbage(&obj.payload, &ptr_map, &garbage_set) {
                confirmed_garbage.push(gi);
            }
        }

        // 7. Make weak refs observe confirmed cyclic garbage as dead before
        // clearing payloads, then notify weak refs that survived the cycle.
        let collected = confirmed_garbage.len();
        for &gi in &confirmed_garbage {
            PyObjectRef::mark_cycle_cleared(&alive[gi]);
        }
        for &gi in &confirmed_garbage {
            PyObjectRef::notify_cycle_collected_weakrefs(&alive[gi], &candidate_ptrs);
        }
        for &gi in &confirmed_garbage {
            break_cycles(&alive[gi].payload);
        }

        drop(alive);
        collected
    }
}

fn collect_gc_reachable(obj: PyObjectRef, alive: &mut Vec<PyObjectRef>, seen: &mut HashSet<usize>) {
    let ptr = PyObjectRef::as_ptr(&obj) as usize;
    if !seen.insert(ptr) {
        return;
    }
    if !is_gc_payload(&obj.payload) {
        return;
    }
    let extra = gc_intermediate_refs(&obj.payload);
    alive.push(obj);
    for child in extra {
        collect_gc_reachable(child, alive, seen);
    }
}

fn is_gc_payload(payload: &PyObjectPayload) -> bool {
    matches!(
        payload,
        PyObjectPayload::Instance(_)
            | PyObjectPayload::List(_)
            | PyObjectPayload::Dict(_)
            | PyObjectPayload::MappingProxy(_)
            | PyObjectPayload::Range(_)
            | PyObjectPayload::Class(_)
            | PyObjectPayload::BoundMethod { .. }
            | PyObjectPayload::BuiltinBoundMethod(_)
            | PyObjectPayload::Set(_)
            | PyObjectPayload::Iterator(_)
            | PyObjectPayload::VecIter(_)
            | PyObjectPayload::WeakValueIter(_)
            | PyObjectPayload::WeakKeyIter(_)
            | PyObjectPayload::RefIter { .. }
            | PyObjectPayload::RevRefIter { .. }
            | PyObjectPayload::DictKeys { .. }
            | PyObjectPayload::DictValues { .. }
            | PyObjectPayload::DictItems { .. }
    )
}

fn gc_add_pyref(obj: &PyObjectRef, ptr_map: &HashMap<usize, usize>, internal_refs: &mut [usize]) {
    let ptr = PyObjectRef::as_ptr(obj) as usize;
    if let Some(&target_idx) = ptr_map.get(&ptr) {
        internal_refs[target_idx] += 1;
    }
}

fn gc_ref_is_garbage(
    obj: &PyObjectRef,
    ptr_map: &HashMap<usize, usize>,
    garbage_set: &HashSet<usize>,
) -> bool {
    let ptr = PyObjectRef::as_ptr(obj) as usize;
    match ptr_map.get(&ptr) {
        Some(target_idx) => garbage_set.contains(target_idx),
        None => true,
    }
}

fn gc_intermediate_refs(payload: &PyObjectPayload) -> Vec<PyObjectRef> {
    match payload {
        PyObjectPayload::Instance(inst) => {
            let mut refs: Vec<PyObjectRef> = inst.attrs.read().values().cloned().collect();
            refs.push(inst.class.clone());
            if let Some(storage) = inst.dict_storage.as_ref() {
                refs.extend(dict_storage_refs(storage));
            }
            if inst.attrs.read().contains_key("__weakref_ref__") {
                if let Some(callback) = inst.attrs.read().get("__weakref_callback__").cloned() {
                    refs.push(callback);
                }
            }
            refs
        }
        PyObjectPayload::List(items) => items.read().iter().cloned().collect(),
        PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => dict_storage_refs(map),
        PyObjectPayload::Range(rd) => {
            let mut refs = Vec::new();
            if let Some(obj) = &rd.start_obj {
                refs.push(obj.clone());
            }
            if let Some(obj) = &rd.stop_obj {
                refs.push(obj.clone());
            }
            if let Some(obj) = &rd.step_obj {
                refs.push(obj.clone());
            }
            refs
        }
        PyObjectPayload::Class(cd) => class_refs(cd),
        PyObjectPayload::BoundMethod { receiver, method } => {
            vec![receiver.clone(), method.clone()]
        }
        PyObjectPayload::BuiltinBoundMethod(data) => vec![data.receiver.clone()],
        PyObjectPayload::Set(items) => set_storage_refs(items),
        PyObjectPayload::DictKeys { map, owner }
        | PyObjectPayload::DictValues { map, owner }
        | PyObjectPayload::DictItems { map, owner } => {
            let mut refs = dict_storage_refs(map);
            if let Some(owner) = owner {
                refs.push(owner.clone());
            }
            refs
        }
        PyObjectPayload::RefIter { source, .. } | PyObjectPayload::RevRefIter { source, .. } => {
            vec![source.clone()]
        }
        PyObjectPayload::VecIter(data) => data.items.clone(),
        PyObjectPayload::WeakValueIter(data) => data
            .entries
            .iter()
            .flat_map(|(key, ref_obj)| [key.clone(), ref_obj.clone()])
            .collect(),
        PyObjectPayload::WeakKeyIter(data) => data
            .entries
            .iter()
            .flat_map(|(ref_obj, value)| [ref_obj.clone(), value.clone()])
            .collect(),
        PyObjectPayload::Iterator(iter_data) => {
            let data = iter_data.read();
            iterator_refs(&data)
        }
        _ => Vec::new(),
    }
}

fn dict_storage_refs(map: &Rc<PyCell<FxHashKeyMap>>) -> Vec<PyObjectRef> {
    let read = map.read();
    let mut refs = Vec::with_capacity(read.len() * 2);
    for (key, value) in read.iter() {
        hashable_key_refs(key, &mut refs);
        refs.push(value.clone());
    }
    refs
}

fn set_storage_refs(map: &Rc<PyCell<FxHashKeyFlatMap>>) -> Vec<PyObjectRef> {
    let read = map.read();
    let mut refs = Vec::with_capacity(read.len() * 2);
    for (key, value) in read.iter() {
        hashable_key_refs(key, &mut refs);
        refs.push(value.clone());
    }
    refs
}

fn class_refs(cd: &ClassData) -> Vec<PyObjectRef> {
    let mut refs = Vec::new();
    refs.extend(cd.bases.iter().cloned());
    refs.extend(cd.namespace.read().values().cloned());
    refs.extend(cd.mro.iter().cloned());
    if let Some(metaclass) = &cd.metaclass {
        refs.push(metaclass.clone());
    }
    refs.extend(cd.method_cache.read().values().filter_map(|v| v.clone()));
    refs.extend(cd.method_vtable.read().values().cloned());
    refs
}

fn hashable_key_refs(key: &HashableKey, refs: &mut Vec<PyObjectRef>) {
    match key {
        HashableKey::Tuple(items) => {
            for item in items.iter() {
                hashable_key_refs(item, refs);
            }
        }
        HashableKey::FrozenSet(items) => {
            for item in items.iter() {
                hashable_key_refs(item, refs);
            }
        }
        HashableKey::Identity(_, obj) => refs.push(obj.clone()),
        HashableKey::Custom { object, .. } => refs.push(object.clone()),
        _ => {}
    }
}

fn iterator_refs(data: &IteratorData) -> Vec<PyObjectRef> {
    match data {
        IteratorData::List { items, .. }
        | IteratorData::Tuple { items, .. }
        | IteratorData::Cycle { items, .. }
        | IteratorData::Chain { sources: items, .. } => items.clone(),
        IteratorData::Enumerate {
            source,
            cached_tuple,
            ..
        } => {
            let mut refs = vec![source.clone()];
            if let Some(tuple) = cached_tuple {
                refs.push(tuple.clone());
            }
            refs
        }
        IteratorData::Zip {
            sources,
            cached_tuple,
            items_buf,
            ..
        } => {
            let mut refs = sources.clone();
            if let Some(tuple) = cached_tuple {
                refs.push(tuple.clone());
            }
            refs.extend(items_buf.iter().cloned());
            refs
        }
        IteratorData::MapOne { func, source }
        | IteratorData::Filter { func, source }
        | IteratorData::FilterFalse { func, source }
        | IteratorData::TakeWhile { func, source, .. }
        | IteratorData::DropWhile { func, source, .. }
        | IteratorData::Starmap { func, source } => vec![func.clone(), source.clone()],
        IteratorData::Map { func, sources } => {
            let mut refs = Vec::with_capacity(sources.len() + 1);
            refs.push(func.clone());
            refs.extend(sources.iter().cloned());
            refs
        }
        IteratorData::Sentinel {
            callable, sentinel, ..
        } => vec![callable.clone(), sentinel.clone()],
        IteratorData::SeqIter { obj, .. } => vec![obj.clone()],
        IteratorData::Repeat { item, .. } => vec![item.clone()],
        IteratorData::Tee { source, buffer, .. } => {
            let mut refs = vec![source.read().clone()];
            refs.extend(buffer.read().iter().cloned());
            refs
        }
        IteratorData::DictEntries {
            source,
            owner,
            cached_tuple,
            ..
        } => {
            let mut refs = dict_storage_refs(source);
            if let Some(owner) = owner {
                refs.push(owner.clone());
            }
            if let Some(tuple) = cached_tuple {
                refs.push(tuple.clone());
            }
            refs
        }
        IteratorData::DictKeys { keys, .. } => keys.clone(),
        IteratorData::HeldIter { iter, owner } => {
            let mut refs = vec![iter.clone()];
            if let Some(owner) = owner {
                refs.push(owner.clone());
            }
            refs
        }
        IteratorData::Range { .. } | IteratorData::Str { .. } | IteratorData::Count { .. } => {
            Vec::new()
        }
    }
}

/// Count references from a payload to other tracked objects.
fn count_internal_refs(
    payload: &PyObjectPayload,
    ptr_map: &HashMap<usize, usize>,
    internal_refs: &mut [usize],
) {
    match payload {
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            for attr_val in attrs.values() {
                gc_add_pyref(attr_val, ptr_map, internal_refs);
            }
            if let Some(storage) = inst.dict_storage.as_ref() {
                for item in dict_storage_refs(storage) {
                    gc_add_pyref(&item, ptr_map, internal_refs);
                }
            }
            if attrs.contains_key("__weakref_ref__") {
                if let Some(callback) = attrs.get("__weakref_callback__") {
                    gc_add_pyref(&callback, ptr_map, internal_refs);
                }
            }
            gc_add_pyref(&inst.class, ptr_map, internal_refs);
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                gc_add_pyref(item, ptr_map, internal_refs);
            }
        }
        _ => {
            for item in gc_intermediate_refs(payload) {
                gc_add_pyref(&item, ptr_map, internal_refs);
            }
        }
    }
}

/// Verify that all references from a payload point to objects in the garbage set.
fn verify_all_refs_in_garbage(
    payload: &PyObjectPayload,
    ptr_map: &HashMap<usize, usize>,
    garbage_set: &HashSet<usize>,
) -> bool {
    match payload {
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            for attr_val in attrs.values() {
                if !gc_ref_is_garbage(attr_val, ptr_map, garbage_set) {
                    return false;
                }
            }
            if let Some(storage) = inst.dict_storage.as_ref() {
                for item in dict_storage_refs(storage) {
                    if !gc_ref_is_garbage(&item, ptr_map, garbage_set) {
                        return false;
                    }
                }
            }
            true
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                if !gc_ref_is_garbage(item, ptr_map, garbage_set) {
                    return false;
                }
            }
            true
        }
        _ => {
            for item in gc_intermediate_refs(payload) {
                if !gc_ref_is_garbage(&item, ptr_map, garbage_set) {
                    return false;
                }
            }
            true
        }
    }
}

/// Break cycles by clearing contents of a garbage object.
fn break_cycles(payload: &PyObjectPayload) {
    match payload {
        PyObjectPayload::Instance(inst) => {
            if inst.attrs.read().contains_key("__weakref_ref__") {
                return;
            }
            inst.finalizer_state.set(inst.finalizer_state.get() | 2);
            inst.attrs.write().clear();
            if let Some(storage) = inst.dict_storage.as_ref() {
                storage.write().clear();
            }
        }
        PyObjectPayload::List(items) => {
            items.write().clear();
        }
        PyObjectPayload::Dict(map) => {
            map.write().clear();
        }
        PyObjectPayload::Class(cd) => {
            cd.namespace.write().clear();
            cd.method_cache.write().clear();
            cd.method_vtable.write().clear();
            cd.subclasses.write().clear();
        }
        PyObjectPayload::Set(items) => {
            items.write().clear();
        }
        PyObjectPayload::DictKeys { owner, .. }
        | PyObjectPayload::DictValues { owner, .. }
        | PyObjectPayload::DictItems { owner, .. } => {
            let _ = owner;
        }
        _ => {}
    }
}

/// Track object for GC cycle collection.
#[inline(always)]
pub(super) fn track_object(obj: &PyObjectRef) {
    unsafe {
        (*TRACKED_OBJECTS.0.get()).push(PyObjectRef::downgrade(obj));
    }
}
