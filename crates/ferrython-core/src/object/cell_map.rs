//! Shared object cells and FxHash-backed object maps.

use crate::types::{hash_frozenset_key_iter, HashableKey};
use compact_str::CompactString;
use indexmap::IndexMap;
use rustc_hash::FxHasher;
use std::cell::{Cell, UnsafeCell};
use std::fmt;
use std::hash::BuildHasherDefault;
use std::rc::Rc;

use super::PyObjectRef;

/// Zero-overhead interior mutability cell for GIL-semantics interpreter.
/// Provides the same `.read()` / `.write()` / `.data_ptr()` API as parking_lot::RwLock.
///
/// SAFETY: Ferrython uses GIL semantics (single-threaded Python execution).
pub struct PyCell<T>(UnsafeCell<T>);

unsafe impl<T> Send for PyCell<T> {}
unsafe impl<T> Sync for PyCell<T> {}

impl<T> PyCell<T> {
    #[inline(always)]
    pub fn new(val: T) -> Self {
        Self(UnsafeCell::new(val))
    }

    #[inline(always)]
    pub fn read(&self) -> PyCellRef<'_, T> {
        PyCellRef(unsafe { &*self.0.get() })
    }

    #[inline(always)]
    pub fn write(&self) -> PyCellMut<'_, T> {
        PyCellMut(unsafe { &mut *self.0.get() })
    }

    #[inline(always)]
    pub fn data_ptr(&self) -> *mut T {
        self.0.get()
    }
}

impl<T: Clone> Clone for PyCell<T> {
    fn clone(&self) -> Self {
        Self::new(unsafe { &*self.0.get() }.clone())
    }
}

impl<T: fmt::Debug> fmt::Debug for PyCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unsafe { &*self.0.get() }.fmt(f)
    }
}

/// Read guard for PyCell — Deref to &T (zero-cost wrapper).
pub struct PyCellRef<'a, T>(&'a T);

impl<T> std::ops::Deref for PyCellRef<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        self.0
    }
}

/// Write guard for PyCell — DerefMut to &mut T (zero-cost wrapper).
pub struct PyCellMut<'a, T>(&'a mut T);

impl<T> std::ops::Deref for PyCellMut<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        self.0
    }
}

impl<T> std::ops::DerefMut for PyCellMut<'_, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        self.0
    }
}

/// FxHash build hasher — faster than SipHash for short interpreter keys.
pub type FxBuildHasher = BuildHasherDefault<FxHasher>;

/// Attribute map using FxHash instead of SipHash.
pub type FxAttrMap = IndexMap<CompactString, PyObjectRef, FxBuildHasher>;

/// Dict/Set map using FxHash for fast key lookups.
pub type FxHashKeyMap = IndexMap<HashableKey, PyObjectRef, FxBuildHasher>;

/// Flat hash map for Set — no insertion-order tracking.
pub type FxHashKeyFlatMap = std::collections::HashMap<HashableKey, PyObjectRef, FxBuildHasher>;

/// Create a new empty FxHashKeyMap.
#[inline]
pub fn new_fx_hashkey_map() -> FxHashKeyMap {
    IndexMap::with_hasher(FxBuildHasher::default())
}

/// Create a new empty FxHashKeyFlatMap for sets.
#[inline]
pub fn new_fx_hashkey_flatmap() -> FxHashKeyFlatMap {
    FxHashKeyFlatMap::with_hasher(FxBuildHasher::default())
}

/// Create a new FxHashKeyFlatMap with pre-allocated capacity.
#[inline]
pub fn new_fx_hashkey_flatmap_with_capacity(cap: usize) -> FxHashKeyFlatMap {
    FxHashKeyFlatMap::with_capacity_and_hasher(cap, FxBuildHasher::default())
}

/// Convert a SipHash IndexMap to FxHashKeyMap.
#[inline]
pub fn to_fx_hashkey_map(map: IndexMap<HashableKey, PyObjectRef>) -> FxHashKeyMap {
    map.into_iter().collect()
}

#[derive(Clone)]
pub struct FrozenSetData {
    pub items: FxHashKeyMap,
    pub hash_cache: Cell<Option<i64>>,
}

impl FrozenSetData {
    #[inline]
    pub fn new(items: FxHashKeyMap) -> Self {
        Self {
            items,
            hash_cache: Cell::new(None),
        }
    }

    #[inline]
    pub fn py_hash(&self) -> i64 {
        if let Some(cached) = self.hash_cache.get() {
            return cached;
        }
        let hash = hash_frozenset_key_iter(self.items.keys(), self.items.len());
        self.hash_cache.set(Some(hash));
        hash
    }
}

impl std::ops::Deref for FrozenSetData {
    type Target = FxHashKeyMap;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

/// Shared attribute map behind Rc<PyCell>.
pub type SharedFxAttrMap = Rc<PyCell<FxAttrMap>>;

/// Convert a SipHash IndexMap to SharedFxAttrMap.
#[inline]
pub fn to_shared_fx(attrs: IndexMap<CompactString, PyObjectRef>) -> SharedFxAttrMap {
    Rc::new(PyCell::new(attrs.into_iter().collect()))
}

/// Create a new empty SharedFxAttrMap.
#[inline]
pub fn new_shared_fx() -> SharedFxAttrMap {
    Rc::new(PyCell::new(FxAttrMap::default()))
}
