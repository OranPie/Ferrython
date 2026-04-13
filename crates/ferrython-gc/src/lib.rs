//! Ferrython garbage collector — hybrid ref counting + cycle collector.
//!
//! Primary memory management uses `Rc<PyObject>` (reference counting).
//! This module tracks allocations and provides the Python `gc` module API.
//!
//! # Design
//!
//! - `Rc` handles acyclic objects automatically via reference counting
//! - Cycle detection uses a registered callback from ferrython-core
//! - Threshold-based trigger: after N allocations → `collect()`
//! - Three generations mirroring CPython: gen0 (young), gen1, gen2
//!
//! # Performance
//!
//! All counters use thread-local `Cell<u64>` instead of atomics — Ferrython
//! is a single-threaded (GIL) interpreter, so atomics are pure overhead.

use std::sync::Mutex;

// ── GC state — static UnsafeCell (no TLS overhead, single-threaded interpreter) ──

struct GcState {
    enabled: bool,
    allocation_count: u64,
    collection_count: u64,
    threshold_gen0: u64,
    threshold_gen1: u64,
    threshold_gen2: u64,
    gen0_collections: u64,
    gen1_collections: u64,
}

struct GcHolder(std::cell::UnsafeCell<GcState>);
unsafe impl Sync for GcHolder {}

static GC: GcHolder = GcHolder(std::cell::UnsafeCell::new(GcState {
    enabled: true,
    allocation_count: 0,
    collection_count: 0,
    threshold_gen0: 700,
    threshold_gen1: 10,
    threshold_gen2: 10,
    gen0_collections: 0,
    gen1_collections: 0,
}));

// Cycle collection callback — registered once at startup (Mutex is fine here)
static CYCLE_COLLECTOR: Mutex<Option<Box<dyn Fn() -> usize + Send>>> = Mutex::new(None);

/// Register a cycle collection callback. Called by ferrython-core during init.
pub fn register_cycle_collector<F: Fn() -> usize + Send + 'static>(f: F) {
    *CYCLE_COLLECTOR.lock().unwrap() = Some(Box::new(f));
}

// ── Public API ──

/// Notify the GC that an object was allocated. Returns `true` if a
/// generation-0 collection should be triggered.
#[inline(always)]
pub fn notify_alloc() -> bool {
    unsafe {
        let gc = &mut *GC.0.get();
        if !gc.enabled { return false; }
        gc.allocation_count += 1;
        gc.allocation_count >= gc.threshold_gen0
    }
}

/// Run a garbage collection cycle. Returns the number of unreachable
/// objects found via cycle detection.
///
/// Resets allocation counter and increments generation counters to match
/// CPython's generational promotion logic.
pub fn collect() -> usize {
    unsafe {
        let gc = &mut *GC.0.get();
        gc.allocation_count = 0;
        gc.collection_count += 1;
        gc.gen0_collections += 1;
        if gc.gen0_collections >= gc.threshold_gen1 {
            gc.gen0_collections = 0;
            gc.gen1_collections += 1;
            if gc.gen1_collections >= gc.threshold_gen2 {
                gc.gen1_collections = 0;
            }
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
    unsafe { (*GC.0.get()).enabled = true; }
}

/// Disable garbage collection.
pub fn disable() {
    unsafe { (*GC.0.get()).enabled = false; }
}

/// Return whether GC is currently enabled.
pub fn is_enabled() -> bool {
    unsafe { (*GC.0.get()).enabled }
}

/// Get the current collection thresholds as `(gen0, gen1, gen2)`.
pub fn get_threshold() -> (u64, u64, u64) {
    unsafe {
        let gc = &*GC.0.get();
        (gc.threshold_gen0, gc.threshold_gen1, gc.threshold_gen2)
    }
}

/// Set the collection thresholds `(gen0, gen1, gen2)`.
pub fn set_threshold(gen0: u64, gen1: u64, gen2: u64) {
    unsafe {
        let gc = &mut *GC.0.get();
        gc.threshold_gen0 = gen0;
        gc.threshold_gen1 = gen1;
        gc.threshold_gen2 = gen2;
    }
}

/// Get GC statistics: `(alloc_count, total_collections, enabled)`.
pub fn get_stats() -> GcStats {
    unsafe {
        let gc = &*GC.0.get();
        GcStats {
            allocations: gc.allocation_count,
            collections: gc.collection_count,
            enabled: gc.enabled,
            threshold: (gc.threshold_gen0, gc.threshold_gen1, gc.threshold_gen2),
        }
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