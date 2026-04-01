//! Ferrython garbage collector — hybrid ref counting + cycle collector.
//!
//! # Current Status
//!
//! Primary memory management uses `Arc<PyObject>` (reference counting).
//! This module provides the API surface for future cycle detection on Instance objects.
//!
//! # Design
//!
//! - Cycle detection only needed for `Instance` objects (user-defined classes)
//! - Trigger after N allocations or on explicit `gc.collect()`
//! - Mark-and-sweep on Instance graph only (primitives are acyclic by construction)

use std::sync::atomic::{AtomicU64, Ordering};

static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static COLLECTION_THRESHOLD: AtomicU64 = AtomicU64::new(10_000);

/// Notify the GC that an object was allocated. Returns `true` if collection
/// should be triggered (allocation count exceeded threshold).
pub fn notify_alloc() -> bool {
    let count = ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
    count >= COLLECTION_THRESHOLD.load(Ordering::Relaxed)
}

/// Run a garbage collection cycle. Returns the number of objects collected.
///
/// Currently a no-op — cycle collection is not yet implemented.
pub fn collect() -> usize {
    ALLOCATION_COUNT.store(0, Ordering::Relaxed);
    0 // No cycles collected yet
}

/// Check whether a collection should be triggered.
pub fn should_collect() -> bool {
    ALLOCATION_COUNT.load(Ordering::Relaxed) >= COLLECTION_THRESHOLD.load(Ordering::Relaxed)
}

/// Set the allocation threshold that triggers automatic collection.
pub fn set_threshold(threshold: u64) {
    COLLECTION_THRESHOLD.store(threshold, Ordering::Relaxed);
}

/// Get the current allocation count and threshold.
pub fn get_stats() -> (u64, u64) {
    (
        ALLOCATION_COUNT.load(Ordering::Relaxed),
        COLLECTION_THRESHOLD.load(Ordering::Relaxed),
    )
}