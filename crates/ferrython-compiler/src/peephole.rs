//! Peephole optimizer — bytecode-level optimizations applied after compilation.
//!
//! Implements a subset of CPython's peephole optimizer:
//! - Constant folding: `LOAD_CONST a; LOAD_CONST b; BINARY_ADD` → `LOAD_CONST (a+b)`
//! - Dead store elimination: `LOAD_CONST; POP_TOP` → `NOP; NOP`
//! - Jump chain collapse: `JUMP x` where `x` is `JUMP y` → `JUMP y`
//! - Conditional jump over unconditional jump simplification

use ferrython_bytecode::code::{CodeObject, ConstantValue};
use ferrython_bytecode::opcode::{Instruction, Opcode};

/// Run all peephole optimizations on a code object (and recursively on nested code objects).
pub fn optimize(code: &mut CodeObject) {
    // Recursively optimize nested code objects (functions, classes, comprehensions)
    for constant in &mut code.constants {
        if let ConstantValue::Code(inner) = constant {
            optimize(inner);
        }
    }

    // Run passes until no more changes (fixed-point iteration)
    let mut changed = true;
    while changed {
        changed = false;
        changed |= fold_constants(code);
        changed |= eliminate_dead_stores(code);
        changed |= eliminate_dead_code(code);
        changed |= collapse_jump_chains(code);
    }

    // Final cleanup: remove NOP instructions and fix jump targets
    remove_nops(code);
}

/// Constant folding: replace `LOAD_CONST a; LOAD_CONST b; BINARY_OP` with `LOAD_CONST result`.
fn fold_constants(code: &mut CodeObject) -> bool {
    let mut changed = false;
    let n = code.instructions.len();
    if n < 3 { return false; }

    let mut i = 0;
    while i + 2 < n {
        let a_op = code.instructions[i].op;
        let b_op = code.instructions[i + 1].op;
        let op_op = code.instructions[i + 2].op;
        let a_arg = code.instructions[i].arg as usize;
        let b_arg = code.instructions[i + 1].arg as usize;

        if a_op == Opcode::LoadConst && b_op == Opcode::LoadConst {
            let result = {
                let ca = &code.constants[a_arg];
                let cb = &code.constants[b_arg];
                match op_op {
                    Opcode::BinaryAdd => fold_add(ca, cb),
                    Opcode::BinarySubtract => fold_sub(ca, cb),
                    Opcode::BinaryMultiply => fold_mul(ca, cb),
                    Opcode::BinaryTrueDivide => fold_truediv(ca, cb),
                    Opcode::BinaryFloorDivide => fold_floordiv(ca, cb),
                    Opcode::BinaryModulo => fold_mod(ca, cb),
                    Opcode::BinaryPower => fold_pow(ca, cb),
                    Opcode::BinaryLshift => fold_lshift(ca, cb),
                    Opcode::BinaryRshift => fold_rshift(ca, cb),
                    Opcode::BinaryAnd => fold_bitand(ca, cb),
                    Opcode::BinaryOr => fold_bitor(ca, cb),
                    Opcode::BinaryXor => fold_bitxor(ca, cb),
                    _ => None,
                }
            };

            if let Some(folded) = result {
                let idx = intern_constant(code, folded);
                code.instructions[i] = Instruction::new(Opcode::LoadConst, idx as u32);
                code.instructions[i + 1] = Instruction::simple(Opcode::Nop);
                code.instructions[i + 2] = Instruction::simple(Opcode::Nop);
                changed = true;
                i += 3;
                continue;
            }
        }

        // Fold unary: LOAD_CONST a; UNARY_OP → LOAD_CONST result
        if a_op == Opcode::LoadConst {
            let next_op = code.instructions[i + 1].op;
            let result = {
                let ca = &code.constants[a_arg];
                match next_op {
                    Opcode::UnaryNegative => fold_neg(ca),
                    Opcode::UnaryPositive => fold_pos(ca),
                    Opcode::UnaryNot => fold_not(ca),
                    Opcode::UnaryInvert => fold_invert(ca),
                    _ => None,
                }
            };

            if let Some(folded) = result {
                let idx = intern_constant(code, folded);
                code.instructions[i] = Instruction::new(Opcode::LoadConst, idx as u32);
                code.instructions[i + 1] = Instruction::simple(Opcode::Nop);
                changed = true;
                i += 2;
                continue;
            }
        }

        i += 1;
    }

    changed
}

/// Eliminate dead stores: `LOAD_CONST; POP_TOP` → `NOP; NOP`
fn eliminate_dead_stores(code: &mut CodeObject) -> bool {
    let mut changed = false;
    let n = code.instructions.len();
    let mut i = 0;
    while i + 1 < n {
        let a = &code.instructions[i];
        let b = &code.instructions[i + 1];
        // LOAD_CONST followed by POP_TOP with no side effects
        if a.op == Opcode::LoadConst && b.op == Opcode::PopTop {
            code.instructions[i] = Instruction::simple(Opcode::Nop);
            code.instructions[i + 1] = Instruction::simple(Opcode::Nop);
            changed = true;
            i += 2;
            continue;
        }
        i += 1;
    }
    changed
}

/// Dead code elimination: NOP-out instructions after unconditional jumps/returns/raises
/// until the next jump target or exception handler.
fn eliminate_dead_code(code: &mut CodeObject) -> bool {
    let n = code.instructions.len();
    if n < 2 { return false; }

    // Collect all possible jump targets (any instruction that could be branched to)
    let mut live_targets = std::collections::HashSet::new();
    for instr in &code.instructions {
        if is_jump(instr.op) {
            live_targets.insert(instr.arg as usize);
        }
    }

    let mut changed = false;
    let mut dead = false;
    for i in 0..n {
        if dead {
            // This instruction is unreachable — but stop if it's a jump target
            if live_targets.contains(&i) {
                dead = false;
            } else if code.instructions[i].op != Opcode::Nop {
                code.instructions[i] = Instruction::new(Opcode::Nop, 0);
                changed = true;
            }
        }

        if !dead {
            match code.instructions[i].op {
                Opcode::ReturnValue | Opcode::JumpAbsolute
                | Opcode::RaiseVarargs | Opcode::JumpForward => {
                    dead = true;
                }
                _ => {}
            }
        }
    }
    changed
}

/// Collapse jump chains: if a jump target is itself a jump, redirect to the final target.
fn collapse_jump_chains(code: &mut CodeObject) -> bool {
    let mut changed = false;
    let n = code.instructions.len();
    for i in 0..n {
        let instr = &code.instructions[i];
        match instr.op {
            Opcode::JumpAbsolute | Opcode::JumpForward => {
                let target = resolve_jump_target(instr);
                if target < n {
                    let target_instr = &code.instructions[target];
                    if target_instr.op == Opcode::JumpAbsolute {
                        let final_target = target_instr.arg;
                        if code.instructions[i].arg != final_target {
                            code.instructions[i] = Instruction::new(Opcode::JumpAbsolute, final_target);
                            changed = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

/// Remove NOP instructions and rewrite jump targets accordingly.
fn remove_nops(code: &mut CodeObject) {
    let n = code.instructions.len();
    if n == 0 { return; }

    // Build mapping: old_index → new_index (after NOP removal)
    let mut old_to_new: Vec<usize> = Vec::with_capacity(n);
    let mut new_idx = 0usize;
    for instr in &code.instructions {
        old_to_new.push(new_idx);
        if instr.op != Opcode::Nop {
            new_idx += 1;
        }
    }

    // Rewrite jump targets
    for instr in &mut code.instructions {
        if is_jump(instr.op) {
            let target = instr.arg as usize;
            if target < old_to_new.len() {
                instr.arg = old_to_new[target] as u32;
            }
        }
    }

    // Rewrite line number table
    for entry in &mut code.line_number_table {
        let old_idx = entry.0 as usize;
        if old_idx < old_to_new.len() {
            entry.0 = old_to_new[old_idx] as u32;
        }
    }

    // Remove NOPs
    code.instructions.retain(|i| i.op != Opcode::Nop);
}

// ── Helpers ──

fn resolve_jump_target(instr: &Instruction) -> usize {
    instr.arg as usize
}

fn is_jump(op: Opcode) -> bool {
    matches!(op,
        Opcode::JumpForward | Opcode::JumpAbsolute |
        Opcode::PopJumpIfFalse | Opcode::PopJumpIfTrue |
        Opcode::JumpIfFalseOrPop | Opcode::JumpIfTrueOrPop |
        Opcode::ForIter | Opcode::SetupFinally | Opcode::SetupExcept |
        Opcode::SetupWith | Opcode::SetupAsyncWith
    )
}

fn intern_constant(code: &mut CodeObject, val: ConstantValue) -> usize {
    // Reuse existing constant if possible
    for (i, c) in code.constants.iter().enumerate() {
        if c.bit_exact_eq(&val) {
            return i;
        }
    }
    code.constants.push(val);
    code.constants.len() - 1
}

// ── Constant folding operations ──

fn fold_add(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => {
            x.checked_add(*y).map(ConstantValue::Integer)
        }
        (ConstantValue::Float(x), ConstantValue::Float(y)) => {
            Some(ConstantValue::Float(x + y))
        }
        (ConstantValue::Integer(x), ConstantValue::Float(y)) => {
            Some(ConstantValue::Float(*x as f64 + y))
        }
        (ConstantValue::Float(x), ConstantValue::Integer(y)) => {
            Some(ConstantValue::Float(x + *y as f64))
        }
        (ConstantValue::Str(x), ConstantValue::Str(y)) => {
            let mut s = x.to_string();
            s.push_str(y.as_str());
            Some(ConstantValue::Str(s.into()))
        }
        _ => None,
    }
}

fn fold_sub(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => {
            x.checked_sub(*y).map(ConstantValue::Integer)
        }
        (ConstantValue::Float(x), ConstantValue::Float(y)) => Some(ConstantValue::Float(x - y)),
        (ConstantValue::Integer(x), ConstantValue::Float(y)) => Some(ConstantValue::Float(*x as f64 - y)),
        (ConstantValue::Float(x), ConstantValue::Integer(y)) => Some(ConstantValue::Float(x - *y as f64)),
        _ => None,
    }
}

fn fold_mul(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => {
            x.checked_mul(*y).map(ConstantValue::Integer)
        }
        (ConstantValue::Float(x), ConstantValue::Float(y)) => Some(ConstantValue::Float(x * y)),
        (ConstantValue::Integer(x), ConstantValue::Float(y)) => Some(ConstantValue::Float(*x as f64 * y)),
        (ConstantValue::Float(x), ConstantValue::Integer(y)) => Some(ConstantValue::Float(x * *y as f64)),
        // String repetition: "abc" * 3 → "abcabcabc" (limit to reasonable sizes)
        (ConstantValue::Str(s), ConstantValue::Integer(n)) if *n >= 0 && *n <= 20 => {
            Some(ConstantValue::Str(s.repeat(*n as usize).into()))
        }
        (ConstantValue::Integer(n), ConstantValue::Str(s)) if *n >= 0 && *n <= 20 => {
            Some(ConstantValue::Str(s.repeat(*n as usize).into()))
        }
        _ => None,
    }
}

fn fold_truediv(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) if *y != 0 => {
            Some(ConstantValue::Float(*x as f64 / *y as f64))
        }
        (ConstantValue::Float(x), ConstantValue::Float(y)) if *y != 0.0 => {
            Some(ConstantValue::Float(x / y))
        }
        (ConstantValue::Integer(x), ConstantValue::Float(y)) if *y != 0.0 => {
            Some(ConstantValue::Float(*x as f64 / y))
        }
        (ConstantValue::Float(x), ConstantValue::Integer(y)) if *y != 0 => {
            Some(ConstantValue::Float(x / *y as f64))
        }
        _ => None,
    }
}

fn fold_floordiv(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) if *y != 0 => {
            Some(ConstantValue::Integer(x.div_euclid(*y)))
        }
        _ => None,
    }
}

fn fold_mod(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) if *y != 0 => {
            Some(ConstantValue::Integer(x.rem_euclid(*y)))
        }
        _ => None,
    }
}

fn fold_pow(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) if *y >= 0 && *y <= 100 => {
            // Only fold small positive exponents to avoid huge results
            let result = (*x as f64).powi(*y as i32);
            if result.is_finite() && result.abs() <= (i64::MAX as f64) {
                Some(ConstantValue::Integer(result as i64))
            } else {
                None
            }
        }
        (ConstantValue::Float(x), ConstantValue::Float(y)) => {
            let r = x.powf(*y);
            if r.is_finite() { Some(ConstantValue::Float(r)) } else { None }
        }
        _ => None,
    }
}

fn fold_lshift(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) if *y >= 0 && *y < 64 => {
            Some(ConstantValue::Integer(x << y))
        }
        _ => None,
    }
}

fn fold_rshift(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) if *y >= 0 && *y < 64 => {
            Some(ConstantValue::Integer(x >> y))
        }
        _ => None,
    }
}

fn fold_bitand(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => Some(ConstantValue::Integer(x & y)),
        _ => None,
    }
}

fn fold_bitor(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => Some(ConstantValue::Integer(x | y)),
        _ => None,
    }
}

fn fold_bitxor(a: &ConstantValue, b: &ConstantValue) -> Option<ConstantValue> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => Some(ConstantValue::Integer(x ^ y)),
        _ => None,
    }
}

fn fold_neg(a: &ConstantValue) -> Option<ConstantValue> {
    match a {
        ConstantValue::Integer(x) => x.checked_neg().map(ConstantValue::Integer),
        ConstantValue::Float(x) => Some(ConstantValue::Float(-x)),
        _ => None,
    }
}

fn fold_pos(a: &ConstantValue) -> Option<ConstantValue> {
    match a {
        ConstantValue::Integer(_) | ConstantValue::Float(_) => Some(a.clone()),
        _ => None,
    }
}

fn fold_not(a: &ConstantValue) -> Option<ConstantValue> {
    match a {
        ConstantValue::Bool(b) => Some(ConstantValue::Bool(!b)),
        ConstantValue::Integer(n) => Some(ConstantValue::Bool(*n == 0)),
        ConstantValue::None => Some(ConstantValue::Bool(true)),
        _ => None,
    }
}

fn fold_invert(a: &ConstantValue) -> Option<ConstantValue> {
    match a {
        ConstantValue::Integer(x) => Some(ConstantValue::Integer(!x)),
        ConstantValue::Bool(b) => Some(ConstantValue::Integer(if *b { -2 } else { -1 })),
        _ => None,
    }
}
