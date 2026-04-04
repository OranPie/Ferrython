//! Execution profiler — tracks opcode execution counts, function call counts,
//! and wall-clock timing for performance analysis.

use ferrython_bytecode::opcode::Opcode;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Per-opcode execution statistics.
#[derive(Debug, Clone, Default)]
pub struct OpcodeStats {
    /// Total number of times this opcode was executed.
    pub count: u64,
    /// Total wall-clock time spent executing this opcode.
    pub total_time: Duration,
}

/// Execution profiler that accumulates per-opcode and per-function statistics.
///
/// # Usage
///
/// ```ignore
/// let mut profiler = ExecutionProfiler::new();
/// profiler.start_instruction(Opcode::LoadFast);
/// // ... execute opcode ...
/// profiler.end_instruction(Opcode::LoadFast);
/// profiler.report();
/// ```
pub struct ExecutionProfiler {
    enabled: bool,
    opcode_stats: HashMap<u8, OpcodeStats>,
    function_calls: HashMap<String, u64>,
    total_instructions: u64,
    start_time: Instant,
    /// Timestamp of the most recent start_instruction call.
    current_start: Option<Instant>,
}

impl ExecutionProfiler {
    pub fn new() -> Self {
        Self {
            enabled: false,
            opcode_stats: HashMap::new(),
            function_calls: HashMap::new(),
            total_instructions: 0,
            start_time: Instant::now(),
            current_start: None,
        }
    }

    /// Enable or disable profiling. When disabled, all calls are no-ops.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.start_time = Instant::now();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Record the start of an instruction execution.
    #[inline]
    pub fn start_instruction(&mut self, _op: Opcode) {
        if self.enabled {
            self.current_start = Some(Instant::now());
        }
    }

    /// Record the end of an instruction execution, accumulating stats.
    #[inline]
    pub fn end_instruction(&mut self, op: Opcode) {
        if !self.enabled { return; }
        self.total_instructions += 1;
        let elapsed = self.current_start.map(|s| s.elapsed()).unwrap_or_default();
        let entry = self.opcode_stats.entry(op as u8).or_default();
        entry.count += 1;
        entry.total_time += elapsed;
    }

    /// Record a function call by name.
    pub fn record_call(&mut self, func_name: &str) {
        if !self.enabled { return; }
        *self.function_calls.entry(func_name.to_string()).or_default() += 1;
    }

    /// Reset all accumulated statistics.
    pub fn reset(&mut self) {
        self.opcode_stats.clear();
        self.function_calls.clear();
        self.total_instructions = 0;
        self.start_time = Instant::now();
    }

    /// Total number of instructions executed since profiling started.
    pub fn total_instructions(&self) -> u64 {
        self.total_instructions
    }

    /// Total wall-clock time since profiling was enabled.
    pub fn wall_time(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Print a human-readable profiling report to stdout.
    pub fn report(&self) {
        let wall = self.wall_time();
        println!("╔══════════════════════════════════════════════════════════╗");
        println!("║          Ferrython Execution Profile                    ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║ Total instructions: {:>12}                         ║", self.total_instructions);
        println!("║ Wall clock time:    {:>12.3}ms                      ║", wall.as_secs_f64() * 1000.0);
        if self.total_instructions > 0 {
            let ns_per_op = wall.as_nanos() as f64 / self.total_instructions as f64;
            println!("║ Avg ns/instruction: {:>12.1}                         ║", ns_per_op);
        }
        println!("╠══════════════════════════════════════════════════════════╣");

        // Sort opcodes by total time (descending)
        let mut stats: Vec<_> = self.opcode_stats.iter().collect();
        stats.sort_by(|a, b| b.1.total_time.cmp(&a.1.total_time));

        println!("║ {:24} {:>10} {:>10} {:>8} ║", "Opcode", "Count", "Time(ms)", "%");
        println!("║ {:24} {:>10} {:>10} {:>8} ║", "──────", "─────", "───────", "──");

        let total_ns = wall.as_nanos() as f64;
        for (op_byte, stat) in stats.iter().take(20) {
            let op = opcode_from_byte(**op_byte);
            let pct = if total_ns > 0.0 { stat.total_time.as_nanos() as f64 / total_ns * 100.0 } else { 0.0 };
            println!("║ {:24} {:>10} {:>10.2} {:>7.1}% ║",
                op, stat.count, stat.total_time.as_secs_f64() * 1000.0, pct);
        }

        if !self.function_calls.is_empty() {
            println!("╠══════════════════════════════════════════════════════════╣");
            println!("║ Top called functions:                                   ║");

            let mut calls: Vec<_> = self.function_calls.iter().collect();
            calls.sort_by(|a, b| b.1.cmp(a.1));

            for (name, count) in calls.iter().take(15) {
                let display_name = if name.len() > 35 { &name[..35] } else { name };
                println!("║   {:40} {:>10} ║", display_name, count);
            }
        }

        println!("╚══════════════════════════════════════════════════════════╝");
    }
}

impl Default for ExecutionProfiler {
    fn default() -> Self { Self::new() }
}

/// Convert a raw opcode byte back to a display string.
fn opcode_from_byte(byte: u8) -> String {
    // Use Opcode's Debug representation if possible.
    // Opcode is repr(u8), so we can transmute safely for known values.
    format!("{:?}", unsafe { std::mem::transmute::<u8, Opcode>(byte) })
}
