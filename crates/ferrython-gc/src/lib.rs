//! Ferrython garbage collector — hybrid ref counting + cycle collector.
//!
//! Primary memory management uses `Arc<PyObject>` (reference counting).
//! This module tracks allocations and provides the Python `gc` module API.
//!
//! # Design
//!
//! - `Arc` handles acyclic objects automatically via reference counting
//! - Cycle detection uses a registered callback from ferrython-core
//! - Threshold-based trigger: after N allocations → `collect()`
//! - Three generations mirroring CPython: gen0 (young), gen1, gen2

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

// ── Global GC state (thread-safe via atomics) ──

static ENABLED: AtomicBool = AtomicBool::new(true);
static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static COLLECTION_COUNT: AtomicU64 = AtomicU64::new(0);

// CPython default thresholds: (700, 10, 10) — gen0=700 allocs, gen1=10 gen0 cycles, gen2=10 gen1 cycles
static THRESHOLD_GEN0: AtomicU64 = AtomicU64::new(700);
static THRESHOLD_GEN1: AtomicU64 = AtomicU64::new(10);
static THRESHOLD_GEN2: AtomicU64 = AtomicU64::new(10);

// Generation collection counters
static GEN0_COLLECTIONS: AtomicU64 = AtomicU64::new(0);
static GEN1_COLLECTIONS: AtomicU64 = AtomicU64::new(0);

// Cycle collection callback — registered by ferrython-core
static CYCLE_COLLECTOR: Mutex<Option<Box<dyn Fn() -> usize + Send>>> = Mutex::new(None);

/// Register a cycle collection callback. Called by ferrython-core during init.
pub fn register_cycle_collector<F: Fn() -> usize + Send + 'static>(f: F) {
    *CYCLE_COLLECTOR.lock().unwrap() = Some(Box::new(f));
}

// ── Public API ──

/// Notify the GC that an object was allocated. Returns `true` if a
/// generation-0 collection should be triggered.
#[inline]
pub fn notify_alloc() -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    let count = ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    count >= THRESHOLD_GEN0.load(Ordering::Relaxed)
}

/// Run a garbage collection cycle. Returns the number of unreachable
/// objects found via cycle detection.
///
/// Resets allocation counter and increments generation counters to match
/// CPython's generational promotion logic.
pub fn collect() -> usize {
    let _allocs = ALLOCATION_COUNT.swap(0, Ordering::Relaxed);
    COLLECTION_COUNT.fetch_add(1, Ordering::Relaxed);

    // Generational promotion: gen0 collection happened
    let gen0 = GEN0_COLLECTIONS.fetch_add(1, Ordering::Relaxed) + 1;
    if gen0 >= THRESHOLD_GEN1.load(Ordering::Relaxed) {
        GEN0_COLLECTIONS.store(0, Ordering::Relaxed);
        let gen1 = GEN1_COLLECTIONS.fetch_add(1, Ordering::Relaxed) + 1;
        if gen1 >= THRESHOLD_GEN2.load(Ordering::Relaxed) {
            GEN1_COLLECTIONS.store(0, Ordering::Relaxed);
        }
    }

    // Run the registered cycle collector callback
    if let Ok(guard) = CYCLE_COLLECTOR.lock() {
        if let Some(ref collector) = *guard {
            return collector();
        }
    }
    0
}

/// Enable garbage collection.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Disable garbage collection.
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Return whether GC is currently enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Get the current collection thresholds as `(gen0, gen1, gen2)`.
pub fn get_threshold() -> (u64, u64, u64) {
    (
        THRESHOLD_GEN0.load(Ordering::Relaxed),
        THRESHOLD_GEN1.load(Ordering::Relaxed),
        THRESHOLD_GEN2.load(Ordering::Relaxed),
    )
}

/// Set the collection thresholds `(gen0, gen1, gen2)`.
pub fn set_threshold(gen0: u64, gen1: u64, gen2: u64) {
    THRESHOLD_GEN0.store(gen0, Ordering::Relaxed);
    THRESHOLD_GEN1.store(gen1, Ordering::Relaxed);
    THRESHOLD_GEN2.store(gen2, Ordering::Relaxed);
}

/// Get GC statistics: `(alloc_count, total_collections, enabled)`.
pub fn get_stats() -> GcStats {
    GcStats {
        allocations: ALLOCATION_COUNT.load(Ordering::Relaxed),
        collections: COLLECTION_COUNT.load(Ordering::Relaxed),
        enabled: ENABLED.load(Ordering::Relaxed),
        threshold: get_threshold(),
    }
}

/// Snapshot of GC state for the `gc.get_stats()` Python function.
#[derive(Debug, Clone)]
pub struct GcStats {
    pub allocations: u64,
    pub collections: u64,
    pub enabled: bool,
    pub threshold: (u64, u64, u64),
}