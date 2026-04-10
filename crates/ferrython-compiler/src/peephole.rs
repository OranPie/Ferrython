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
            optimize(std::sync::Arc::make_mut(inner));
        }
    }

    // Run passes until no more changes (fixed-point iteration)
    let mut changed = true;
    while changed {
        changed = false;
        changed |= fold_constants(code);
        changed |= fold_constant_comparisons(code);
        changed |= fold_constant_tuples(code);
        changed |= fold_constant_conditionals(code);
        changed |= eliminate_dead_stores(code);
        changed |= eliminate_dead_code(code);
        changed |= collapse_jump_chains(code);
    }

    // Final cleanup: remove NOP instructions and fix jump targets
    remove_nops(code);

    // Superinstruction fusion (after NOPs are removed, after jump targets are final).
    // This fuses adjacent LoadFast+LoadFast, LoadFast+LoadConst, StoreFast+LoadFast
    // into single instructions, saving one dispatch loop iteration per pair.
    fuse_superinstructions(code);
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

/// Constant comparison folding: `LOAD_CONST a; LOAD_CONST b; COMPARE_OP` → `LOAD_CONST True/False`
fn fold_constant_comparisons(code: &mut CodeObject) -> bool {
    let mut changed = false;
    let n = code.instructions.len();
    if n < 3 { return false; }

    let mut i = 0;
    while i + 2 < n {
        let a_op = code.instructions[i].op;
        let b_op = code.instructions[i + 1].op;
        let cmp_op = code.instructions[i + 2].op;

        if a_op == Opcode::LoadConst && b_op == Opcode::LoadConst && cmp_op == Opcode::CompareOp {
            let a_arg = code.instructions[i].arg as usize;
            let b_arg = code.instructions[i + 1].arg as usize;
            let cmp_arg = code.instructions[i + 2].arg;
            let result = {
                let ca = &code.constants[a_arg];
                let cb = &code.constants[b_arg];
                fold_compare(ca, cb, cmp_arg)
            };
            if let Some(folded) = result {
                let idx = intern_constant(code, ConstantValue::Bool(folded));
                code.instructions[i] = Instruction::new(Opcode::LoadConst, idx as u32);
                code.instructions[i + 1] = Instruction::simple(Opcode::Nop);
                code.instructions[i + 2] = Instruction::simple(Opcode::Nop);
                changed = true;
                i += 3;
                continue;
            }
        }
        i += 1;
    }
    changed
}

/// Constant conditional elimination:
/// - `LOAD_CONST True; POP_JUMP_IF_FALSE target` → NOP; NOP (always falls through)
/// - `LOAD_CONST False; POP_JUMP_IF_FALSE target` → NOP; JUMP_ABSOLUTE target
/// - `LOAD_CONST True; POP_JUMP_IF_TRUE target` → NOP; JUMP_ABSOLUTE target
/// - `LOAD_CONST False; POP_JUMP_IF_TRUE target` → NOP; NOP (always falls through)
fn fold_constant_conditionals(code: &mut CodeObject) -> bool {
    let mut changed = false;
    let n = code.instructions.len();
    if n < 2 { return false; }

    let mut i = 0;
    while i + 1 < n {
        let load = code.instructions[i];
        let jump = code.instructions[i + 1];
        if load.op == Opcode::LoadConst
            && (jump.op == Opcode::PopJumpIfFalse || jump.op == Opcode::PopJumpIfTrue)
        {
            let is_truthy = const_is_truthy(&code.constants[load.arg as usize]);
            if let Some(truthy) = is_truthy {
                let jumps = (jump.op == Opcode::PopJumpIfFalse && !truthy)
                    || (jump.op == Opcode::PopJumpIfTrue && truthy);
                code.instructions[i] = Instruction::simple(Opcode::Nop);
                if jumps {
                    code.instructions[i + 1] = Instruction::new(Opcode::JumpAbsolute, jump.arg);
                } else {
                    code.instructions[i + 1] = Instruction::simple(Opcode::Nop);
                }
                changed = true;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    changed
}

/// Fold constant tuples: `LOAD_CONST a; LOAD_CONST b; ... BUILD_TUPLE n` → `LOAD_CONST (a,b,...)`
/// Only folds when ALL elements are constants. Handles tuples up to 16 elements.
fn fold_constant_tuples(code: &mut CodeObject) -> bool {
    let mut changed = false;
    let n = code.instructions.len();
    let mut i = 0;
    while i < n {
        let instr = code.instructions[i];
        if instr.op == Opcode::BuildTuple {
            let count = instr.arg as usize;
            if count > 0 && count <= 16 && i >= count {
                // Check if all preceding `count` instructions are LOAD_CONST
                let start = i - count;
                let all_const = (0..count).all(|j| code.instructions[start + j].op == Opcode::LoadConst);
                if all_const {
                    let elements: Vec<ConstantValue> = (0..count)
                        .map(|j| code.constants[code.instructions[start + j].arg as usize].clone())
                        .collect();
                    let tuple_val = ConstantValue::Tuple(elements);
                    let idx = intern_constant(code, tuple_val);
                    code.instructions[start] = Instruction::new(Opcode::LoadConst, idx as u32);
                    for j in 1..=count {
                        code.instructions[start + j] = Instruction::simple(Opcode::Nop);
                    }
                    changed = true;
                    i = start + count + 1;
                    continue;
                }
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
        // LOAD_FAST followed by POP_TOP (dead load of local variable)
        if a.op == Opcode::LoadFast && b.op == Opcode::PopTop {
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
            // Only fold small positive exponents to avoid huge results.
            // Use checked integer pow to avoid f64 precision loss for large values.
            if let Some(result) = x.checked_pow(*y as u32) {
                Some(ConstantValue::Integer(result))
            } else {
                None // Too large for i64 — let runtime handle with BigInt
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

fn fold_compare(a: &ConstantValue, b: &ConstantValue, cmp: u32) -> Option<bool> {
    match (a, b) {
        (ConstantValue::Integer(x), ConstantValue::Integer(y)) => {
            Some(match cmp {
                0 => x < y,  // Lt
                1 => x <= y, // Le
                2 => x == y, // Eq
                3 => x != y, // Ne
                4 => x > y,  // Gt
                5 => x >= y, // Ge
                _ => return None,
            })
        }
        (ConstantValue::Float(x), ConstantValue::Float(y)) => {
            Some(match cmp {
                0 => x < y,
                1 => x <= y,
                2 => x == y,
                3 => x != y,
                4 => x > y,
                5 => x >= y,
                _ => return None,
            })
        }
        (ConstantValue::Str(x), ConstantValue::Str(y)) if cmp == 2 || cmp == 3 => {
            let eq = x == y;
            Some(if cmp == 2 { eq } else { !eq })
        }
        (ConstantValue::Bool(x), ConstantValue::Bool(y)) if cmp == 2 || cmp == 3 => {
            let eq = x == y;
            Some(if cmp == 2 { eq } else { !eq })
        }
        (ConstantValue::None, ConstantValue::None) if cmp == 2 => Some(true),
        (ConstantValue::None, ConstantValue::None) if cmp == 3 => Some(false),
        _ => None,
    }
}

fn const_is_truthy(val: &ConstantValue) -> Option<bool> {
    match val {
        ConstantValue::Bool(b) => Some(*b),
        ConstantValue::Integer(n) => Some(*n != 0),
        ConstantValue::Float(f) => Some(*f != 0.0),
        ConstantValue::None => Some(false),
        ConstantValue::Str(s) => Some(!s.is_empty()),
        ConstantValue::Tuple(items) => Some(!items.is_empty()),
        _ => None,
    }
}

/// Fuse adjacent opcode pairs into superinstructions.
/// Must run AFTER remove_nops (jump targets are finalized).
/// Superinstructions pack two small args into one u32: (arg1 << 16) | arg2.
fn fuse_superinstructions(code: &mut CodeObject) {
    let n = code.instructions.len();
    if n < 2 { return; }

    // Collect all jump targets so we don't fuse across them
    let mut jump_targets = vec![false; n];
    for instr in &code.instructions {
        if is_jump(instr.op) {
            let target = instr.arg as usize;
            if target < n {
                jump_targets[target] = true;
            }
        }
    }

    // Phase 1: Mark which positions get fused (second instruction → NOP)
    let mut is_nop = vec![false; n];
    let mut i = 0;
    while i + 1 < n {
        let a = code.instructions[i];
        let b = code.instructions[i + 1];

        // Don't fuse if the second instruction is a jump target
        if jump_targets[i + 1] {
            i += 1;
            continue;
        }

        // 3-way fusion: LoadFast + LoadConst + BinarySubtract → LoadFastLoadConstBinarySub
        if i + 2 < n && !jump_targets[i + 2]
            && a.op == Opcode::LoadFast && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinarySubtract
            && a.arg <= 0xFFFF && b.arg <= 0xFFFF
        {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadConstBinarySub, packed_arg);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            i += 3;
            continue;
        }

        // 3-way fusion: LoadFast + LoadConst + BinaryAdd → LoadFastLoadConstBinaryAdd
        if i + 2 < n && !jump_targets[i + 2]
            && a.op == Opcode::LoadFast && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinaryAdd
            && a.arg <= 0xFFFF && b.arg <= 0xFFFF
        {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadConstBinaryAdd, packed_arg);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            i += 3;
            continue;
        }

        // CompareOp + PopJumpIfFalse → CompareOpPopJumpIfFalse
        // Special encoding: (cmp_op << 24) | jump_target
        if a.op == Opcode::CompareOp && b.op == Opcode::PopJumpIfFalse
            && a.arg <= 255 && b.arg <= 0x00FF_FFFF
        {
            let packed = (a.arg << 24) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::CompareOpPopJumpIfFalse, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        if a.arg > 0xFFFF || b.arg > 0xFFFF {
            i += 1;
            continue;
        }

        let fused = match (a.op, b.op) {
            (Opcode::LoadFast, Opcode::LoadFast) => Some(Opcode::LoadFastLoadFast),
            (Opcode::LoadFast, Opcode::LoadConst) => Some(Opcode::LoadFastLoadConst),
            (Opcode::StoreFast, Opcode::LoadFast) => Some(Opcode::StoreFastLoadFast),
            _ => None,
        };

        if let Some(super_op) = fused {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(super_op, packed_arg);
            is_nop[i + 1] = true;
            i += 2;
        } else {
            i += 1;
        }
    }

    // Phase 2: Build old→new index mapping
    let mut old_to_new: Vec<usize> = Vec::with_capacity(n);
    let mut new_idx: usize = 0;
    for nop in &is_nop {
        old_to_new.push(new_idx);
        if !nop { new_idx += 1; }
    }
    // Sentinel for targets pointing past the end
    let final_len = new_idx;

    if final_len == n { return; } // nothing was fused

    // Phase 3: Rewrite all jump targets
    for instr in &mut code.instructions {
        if instr.op == Opcode::CompareOpPopJumpIfFalse {
            // Jump target is in low 24 bits; cmp_op in high 8 bits
            let cmp_op = instr.arg >> 24;
            let target = (instr.arg & 0x00FF_FFFF) as usize;
            let new_target = if target < old_to_new.len() {
                old_to_new[target] as u32
            } else {
                final_len as u32
            };
            instr.arg = (cmp_op << 24) | new_target;
        } else if is_jump(instr.op) {
            let target = instr.arg as usize;
            if target < old_to_new.len() {
                instr.arg = old_to_new[target] as u32;
            } else {
                instr.arg = final_len as u32;
            }
        }
    }

    // Phase 4: Rewrite line number table
    for entry in &mut code.line_number_table {
        let old_idx = entry.0 as usize;
        if old_idx < old_to_new.len() {
            entry.0 = old_to_new[old_idx] as u32;
        }
    }

    // Phase 5: Remove NOP'd positions
    let mut write = 0;
    for read in 0..n {
        if !is_nop[read] {
            code.instructions[write] = code.instructions[read];
            write += 1;
        }
    }
    code.instructions.truncate(write);
}
