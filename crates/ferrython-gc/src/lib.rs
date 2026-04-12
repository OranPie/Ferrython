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

use std::cell::Cell;
use std::sync::Mutex;

// ── GC state — thread-local Cells (no atomics, single-threaded interpreter) ──

thread_local! {
    static ENABLED: Cell<bool> = Cell::new(true);
    static ALLOCATION_COUNT: Cell<u64> = Cell::new(0);
    static COLLECTION_COUNT: Cell<u64> = Cell::new(0);
    static THRESHOLD_GEN0: Cell<u64> = Cell::new(700);
    static THRESHOLD_GEN1: Cell<u64> = Cell::new(10);
    static THRESHOLD_GEN2: Cell<u64> = Cell::new(10);
    static GEN0_COLLECTIONS: Cell<u64> = Cell::new(0);
    static GEN1_COLLECTIONS: Cell<u64> = Cell::new(0);
}

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
    ENABLED.with(|e| {
        if !e.get() { return false; }
        ALLOCATION_COUNT.with(|c| {
            let count = c.get() + 1;
            c.set(count);
            THRESHOLD_GEN0.with(|t| count >= t.get())
        })
    })
}

/// Run a garbage collection cycle. Returns the number of unreachable
/// objects found via cycle detection.
///
/// Resets allocation counter and increments generation counters to match
/// CPython's generational promotion logic.
pub fn collect() -> usize {
    ALLOCATION_COUNT.with(|c| c.set(0));
    COLLECTION_COUNT.with(|c| c.set(c.get() + 1));

    // Generational promotion: gen0 collection happened
    let gen0 = GEN0_COLLECTIONS.with(|c| { let v = c.get() + 1; c.set(v); v });
    if gen0 >= THRESHOLD_GEN1.with(|t| t.get()) {
        GEN0_COLLECTIONS.with(|c| c.set(0));
        let gen1 = GEN1_COLLECTIONS.with(|c| { let v = c.get() + 1; c.set(v); v });
        if gen1 >= THRESHOLD_GEN2.with(|t| t.get()) {
            GEN1_COLLECTIONS.with(|c| c.set(0));
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
    ENABLED.with(|e| e.set(true));
}

/// Disable garbage collection.
pub fn disable() {
    ENABLED.with(|e| e.set(false));
}

/// Return whether GC is currently enabled.
pub fn is_enabled() -> bool {
    ENABLED.with(|e| e.get())
}

/// Get the current collection thresholds as `(gen0, gen1, gen2)`.
pub fn get_threshold() -> (u64, u64, u64) {
    (
        THRESHOLD_GEN0.with(|t| t.get()),
        THRESHOLD_GEN1.with(|t| t.get()),
        THRESHOLD_GEN2.with(|t| t.get()),
    )
}

/// Set the collection thresholds `(gen0, gen1, gen2)`.
pub fn set_threshold(gen0: u64, gen1: u64, gen2: u64) {
    THRESHOLD_GEN0.with(|t| t.set(gen0));
    THRESHOLD_GEN1.with(|t| t.set(gen1));
    THRESHOLD_GEN2.with(|t| t.set(gen2));
}

/// Get GC statistics: `(alloc_count, total_collections, enabled)`.
pub fn get_stats() -> GcStats {
    GcStats {
        allocations: ALLOCATION_COUNT.with(|c| c.get()),
        collections: COLLECTION_COUNT.with(|c| c.get()),
        enabled: ENABLED.with(|e| e.get()),
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