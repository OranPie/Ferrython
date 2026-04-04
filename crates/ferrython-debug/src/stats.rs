//! Code object statistics — complexity analysis and bytecode metrics.

use ferrython_bytecode::code::{CodeObject, ConstantValue, CodeFlags};
use ferrython_bytecode::opcode::Opcode;
use std::collections::HashMap;

/// Summary statistics for a single code object.
#[derive(Debug)]
pub struct CodeStats {
    pub name: String,
    pub filename: String,
    pub instruction_count: usize,
    pub constant_count: usize,
    pub local_count: usize,
    pub cell_count: usize,
    pub free_count: usize,
    pub is_generator: bool,
    pub is_coroutine: bool,
    pub is_async_generator: bool,
    pub max_stack_depth: u32,
    /// Number of nested code objects (closures, classes, comprehensions).
    pub nested_code_count: usize,
    /// Opcode frequency distribution.
    pub opcode_histogram: HashMap<String, usize>,
    /// Estimated cyclomatic complexity (branches + 1).
    pub cyclomatic_complexity: usize,
}

/// Compute statistics for a code object and all nested code objects.
pub fn code_stats(code: &CodeObject) -> Vec<CodeStats> {
    let mut results = Vec::new();
    collect_stats(code, &mut results);
    results
}

fn collect_stats(code: &CodeObject, out: &mut Vec<CodeStats>) {
    let mut histogram: HashMap<String, usize> = HashMap::new();
    let mut branches = 0usize;

    for instr in &code.instructions {
        *histogram.entry(format!("{:?}", instr.op)).or_default() += 1;

        // Count branch instructions for cyclomatic complexity
        match instr.op {
            Opcode::PopJumpIfTrue | Opcode::PopJumpIfFalse
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::ForIter | Opcode::SetupExcept => {
                branches += 1;
            }
            _ => {}
        }
    }

    let nested = code.constants.iter()
        .filter(|c| matches!(c, ConstantValue::Code(_)))
        .count();

    out.push(CodeStats {
        name: code.name.to_string(),
        filename: code.filename.to_string(),
        instruction_count: code.instructions.len(),
        constant_count: code.constants.len(),
        local_count: code.varnames.len(),
        cell_count: code.cellvars.len(),
        free_count: code.freevars.len(),
        is_generator: code.flags.contains(CodeFlags::GENERATOR),
        is_coroutine: code.flags.contains(CodeFlags::COROUTINE),
        is_async_generator: code.flags.contains(CodeFlags::ASYNC_GENERATOR),
        max_stack_depth: code.max_stack_size,
        nested_code_count: nested,
        opcode_histogram: histogram,
        cyclomatic_complexity: branches + 1,
    });

    // Recurse into nested code objects
    for c in &code.constants {
        if let ConstantValue::Code(nested_code) = c {
            collect_stats(nested_code, out);
        }
    }
}

/// Print a summary of all code objects to stdout.
pub fn print_stats_report(stats: &[CodeStats]) {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║               Bytecode Statistics                       ║");
    println!("╠══════════════════════════════════════════════════════════╣");

    let total_instrs: usize = stats.iter().map(|s| s.instruction_count).sum();
    let total_consts: usize = stats.iter().map(|s| s.constant_count).sum();
    let generators = stats.iter().filter(|s| s.is_generator).count();
    let coroutines = stats.iter().filter(|s| s.is_coroutine).count();

    println!("║ Code objects:      {:>6}                                ║", stats.len());
    println!("║ Total instructions:{:>6}                                ║", total_instrs);
    println!("║ Total constants:   {:>6}                                ║", total_consts);
    if generators > 0 { println!("║ Generators:        {:>6}                                ║", generators); }
    if coroutines > 0 { println!("║ Coroutines:        {:>6}                                ║", coroutines); }
    println!("╠══════════════════════════════════════════════════════════╣");

    // Global opcode histogram
    let mut global_hist: HashMap<String, usize> = HashMap::new();
    for s in stats {
        for (op, count) in &s.opcode_histogram {
            *global_hist.entry(op.clone()).or_default() += count;
        }
    }
    let mut sorted: Vec<_> = global_hist.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    println!("║ Top opcodes:                                            ║");
    for (op, count) in sorted.iter().take(15) {
        let pct = if total_instrs > 0 { *count as f64 / total_instrs as f64 * 100.0 } else { 0.0 };
        println!("║   {:28} {:>8} ({:>5.1}%)          ║", op, count, pct);
    }

    // Most complex functions
    let mut by_complexity: Vec<_> = stats.iter()
        .filter(|s| s.cyclomatic_complexity > 1)
        .collect();
    by_complexity.sort_by(|a, b| b.cyclomatic_complexity.cmp(&a.cyclomatic_complexity));

    if !by_complexity.is_empty() {
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║ Most complex functions:                                 ║");
        for s in by_complexity.iter().take(10) {
            let display = if s.name.len() > 30 { &s.name[..30] } else { &s.name };
            println!("║   {:35} complexity={:<3}       ║", display, s.cyclomatic_complexity);
        }
    }

    println!("╚══════════════════════════════════════════════════════════╝");
}
