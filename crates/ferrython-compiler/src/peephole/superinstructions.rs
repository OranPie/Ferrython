use ferrython_bytecode::code::CodeObject;
use ferrython_bytecode::opcode::{Instruction, Opcode};

/// Fuse adjacent opcode pairs into superinstructions.
/// Must run AFTER remove_nops (jump targets are finalized).
/// Superinstructions pack two small args into one u32: (arg1 << 16) | arg2.
pub(super) fn fuse_superinstructions(code: &mut CodeObject) {
    let n = code.instructions.len();
    if n < 2 {
        return;
    }

    // Collect all jump targets so we don't fuse across them
    let mut jump_targets = vec![false; n];
    for instr in &code.instructions {
        if instr.op.is_jump() {
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
        // Skip instructions already marked as NOP by prior fusions
        if is_nop[i] {
            i += 1;
            continue;
        }
        let a = code.instructions[i];
        let b = code.instructions[i + 1];

        // Don't fuse if the second instruction is a jump target or already NOP
        if jump_targets[i + 1] || is_nop[i + 1] {
            i += 1;
            continue;
        }

        // 4-way fusion: LoadFast + LoadConst + CompareOp + PopJumpIfFalse → LoadFastCompareConstJump
        // Zero-clone: reads local and constant by reference, compares, jumps if false.
        // Encoding: (cmp_op << 28) | (local_idx << 20) | (const_idx << 12) | jump_target
        if i + 3 < n
            && !jump_targets[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 2]
            && !is_nop[i + 3]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::CompareOp
            && code.instructions[i + 3].op == Opcode::PopJumpIfFalse
            && a.arg < 256
            && b.arg < 256
            && code.instructions[i + 2].arg < 16
            && code.instructions[i + 3].arg < 4096
        {
            let cmp_op = code.instructions[i + 2].arg;
            let jump_target = code.instructions[i + 3].arg;
            let packed = (cmp_op << 28) | (a.arg << 20) | (b.arg << 12) | jump_target;
            code.instructions[i] = Instruction::new(Opcode::LoadFastCompareConstJump, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            i += 4;
            continue;
        }

        // 3-way fusion: LoadFast + LoadConst + BinarySubtract → LoadFastLoadConstBinarySub
        if i + 2 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinarySubtract
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadConstBinarySub, packed_arg);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            i += 3;
            continue;
        }

        // 6-way fusion: LoadFast + LoadConst + BinaryMul + LoadConst + BinaryMod + StoreFast → LoadFastMulModStoreFast
        // Hot pattern: x = (x * c1) % c2
        if i + 5 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 3]
            && !jump_targets[i + 4]
            && !is_nop[i + 4]
            && !jump_targets[i + 5]
            && !is_nop[i + 5]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinaryMultiply
            && code.instructions[i + 3].op == Opcode::LoadConst
            && code.instructions[i + 4].op == Opcode::BinaryModulo
            && code.instructions[i + 5].op == Opcode::StoreFast
            && a.arg <= 0xFF
            && b.arg <= 0xFF
            && code.instructions[i + 3].arg <= 0xFF
            && code.instructions[i + 5].arg <= 0xFF
        {
            let const2_idx = code.instructions[i + 3].arg;
            let store_idx = code.instructions[i + 5].arg;
            let packed = (a.arg << 24) | (b.arg << 16) | (const2_idx << 8) | store_idx;
            code.instructions[i] = Instruction::new(Opcode::LoadFastMulModStoreFast, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            is_nop[i + 4] = true;
            is_nop[i + 5] = true;
            i += 6;
            continue;
        }

        // 4-way fusion: LoadFast + LoadConst + BinaryAdd + StoreFast → LoadFastLoadConstBinaryAddStoreFast
        if i + 3 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 3]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinaryAdd
            && code.instructions[i + 3].op == Opcode::StoreFast
            && a.arg <= 0xFF
            && b.arg <= 0xFF
            && code.instructions[i + 3].arg <= 0xFF
        {
            let store_idx = code.instructions[i + 3].arg;
            let packed = (a.arg << 16) | (b.arg << 8) | store_idx;
            code.instructions[i] =
                Instruction::new(Opcode::LoadFastLoadConstBinaryAddStoreFast, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            i += 4;
            continue;
        }

        // 4-way fusion: LoadFast + LoadConst + BinaryMultiply + StoreFast → LoadFastLoadConstBinaryMulStoreFast
        if i + 3 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 3]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinaryMultiply
            && code.instructions[i + 3].op == Opcode::StoreFast
            && a.arg <= 0xFF
            && b.arg <= 0xFF
            && code.instructions[i + 3].arg <= 0xFF
        {
            let store_idx = code.instructions[i + 3].arg;
            let packed = (a.arg << 16) | (b.arg << 8) | store_idx;
            code.instructions[i] =
                Instruction::new(Opcode::LoadFastLoadConstBinaryMulStoreFast, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            i += 4;
            continue;
        }

        // 4-way fusion: LoadFast + LoadConst + BinarySubtract + StoreFast → LoadFastLoadConstBinarySubStoreFast
        if i + 3 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 3]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinarySubtract
            && code.instructions[i + 3].op == Opcode::StoreFast
            && a.arg <= 0xFF
            && b.arg <= 0xFF
            && code.instructions[i + 3].arg <= 0xFF
        {
            let store_idx = code.instructions[i + 3].arg;
            let packed = (a.arg << 16) | (b.arg << 8) | store_idx;
            code.instructions[i] =
                Instruction::new(Opcode::LoadFastLoadConstBinarySubStoreFast, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            i += 4;
            continue;
        }

        // 3-way fusion: LoadFast + LoadConst + BinaryMultiply → LoadFastLoadConstBinaryMul
        if i + 2 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinaryMultiply
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadConstBinaryMul, packed_arg);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            i += 3;
            continue;
        }

        // 3-way fusion: LoadFast + LoadConst + BinaryAdd → LoadFastLoadConstBinaryAdd
        if i + 2 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && code.instructions[i + 2].op == Opcode::BinaryAdd
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadConstBinaryAdd, packed_arg);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            i += 3;
            continue;
        }

        // 4-way fusion: LoadFast + LoadFast + BinaryAdd + StoreFast → LoadFastLoadFastBinaryAddStoreFast
        // Hot accumulator pattern: x = x + i
        if i + 3 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 3]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadFast
            && code.instructions[i + 2].op == Opcode::BinaryAdd
            && code.instructions[i + 3].op == Opcode::StoreFast
            && a.arg <= 0xFF
            && b.arg <= 0xFF
            && code.instructions[i + 3].arg <= 0xFF
        {
            let store_idx = code.instructions[i + 3].arg;
            let packed = (a.arg << 16) | (b.arg << 8) | store_idx;
            code.instructions[i] =
                Instruction::new(Opcode::LoadFastLoadFastBinaryAddStoreFast, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            i += 4;
            continue;
        }

        // 3-way fusion: LoadFast + LoadFast + BinaryAdd → LoadFastLoadFastBinaryAdd
        if i + 2 < n
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadFast
            && code.instructions[i + 2].op == Opcode::BinaryAdd
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed_arg = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadFastBinaryAdd, packed_arg);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            i += 3;
            continue;
        }

        // CompareOp + PopJumpIfFalse → CompareOpPopJumpIfFalse
        // Special encoding: (cmp_op << 24) | jump_target
        if a.op == Opcode::CompareOp
            && b.op == Opcode::PopJumpIfFalse
            && a.arg <= 255
            && b.arg <= 0x00FF_FFFF
        {
            let packed = (a.arg << 24) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::CompareOpPopJumpIfFalse, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // 4-way fusion: LoadFast + LoadFast + CompareOp + PopJumpIfFalse → LoadFastLoadFastCompareJump
        // Zero-clone: reads both locals by reference, no stack ops.
        // Encoding: (cmp_op << 28) | (idx1 << 20) | (idx2 << 12) | jump_target
        if i + 3 < n
            && !jump_targets[i + 2]
            && !jump_targets[i + 3]
            && !is_nop[i + 2]
            && !is_nop[i + 3]
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadFast
            && code.instructions[i + 2].op == Opcode::CompareOp
            && code.instructions[i + 3].op == Opcode::PopJumpIfFalse
            && a.arg < 256
            && b.arg < 256
            && code.instructions[i + 2].arg < 16
            && code.instructions[i + 3].arg < 4096
        {
            let cmp_op = code.instructions[i + 2].arg;
            let jump_target = code.instructions[i + 3].arg;
            let packed = (cmp_op << 28) | (a.arg << 20) | (b.arg << 12) | jump_target;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadFastCompareJump, packed);
            is_nop[i + 1] = true;
            is_nop[i + 2] = true;
            is_nop[i + 3] = true;
            i += 4;
            continue;
        }

        // PopBlock + JumpForward → PopBlockJump
        if a.op == Opcode::PopBlock && b.op == Opcode::JumpForward {
            code.instructions[i] = Instruction::new(Opcode::PopBlockJump, b.arg);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // PopBlock + JumpAbsolute → PopBlockJump
        if a.op == Opcode::PopBlock && b.op == Opcode::JumpAbsolute {
            code.instructions[i] = Instruction::new(Opcode::PopBlockJump, b.arg);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // ForIter + StoreFast → ForIterStoreFast
        // Encoding: (jump_target << 16) | store_idx
        // jump_target must fit in 16 bits
        if a.op == Opcode::ForIter
            && b.op == Opcode::StoreFast
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::ForIterStoreFast, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadGlobal + CallFunction → LoadGlobalCallFunction
        // Encoding: (name_idx << 16) | arg_count
        if a.op == Opcode::LoadGlobal
            && b.op == Opcode::CallFunction
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadGlobalCallFunction, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadGlobal + StoreFast → LoadGlobalStoreFast
        // Stores global directly to local, skipping stack.
        // Encoding: (name_idx << 16) | store_idx
        if a.op == Opcode::LoadGlobal
            && b.op == Opcode::StoreFast
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadGlobalStoreFast, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadConst + LoadFast + CompareOp(in/not_in) + StoreFast → LoadConstLoadFastContainsStoreFast
        // Zero-Arc: reads constant and local by reference, does containment check, stores bool in-place.
        // Encoding: (not_in_flag << 31) | (const_idx << 20) | (fast_idx << 10) | store_idx
        if i + 3 < n
            && a.op == Opcode::LoadConst
            && b.op == Opcode::LoadFast
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
        {
            let c = &code.instructions[i + 2];
            if c.op == Opcode::CompareOp && (c.arg == 6 || c.arg == 7) // in / not in
                && !jump_targets[i + 3] && !is_nop[i + 3]
            {
                let d = &code.instructions[i + 3];
                if d.op == Opcode::StoreFast && a.arg < 1024 && b.arg < 1024 && d.arg < 1024 {
                    let not_in_flag = if c.arg == 7 { 1u32 << 31 } else { 0 };
                    let packed = not_in_flag | (a.arg << 20) | (b.arg << 10) | d.arg;
                    code.instructions[i] =
                        Instruction::new(Opcode::LoadConstLoadFastContainsStoreFast, packed);
                    is_nop[i + 1] = true;
                    is_nop[i + 2] = true;
                    is_nop[i + 3] = true;
                    i += 4;
                    continue;
                }
            }
        }

        // LoadFast + LoadConst + BinarySubscr + StoreFast → LoadFastLoadConstSubscrStoreFast
        // Zero-Arc for container and index: reads local and const by reference.
        // Encoding: (fast_idx << 20) | (const_idx << 10) | store_idx
        if i + 3 < n
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadConst
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
        {
            let c = &code.instructions[i + 2];
            if c.op == Opcode::BinarySubscr && !jump_targets[i + 3] && !is_nop[i + 3] {
                let d = &code.instructions[i + 3];
                if d.op == Opcode::StoreFast && a.arg < 1024 && b.arg < 1024 && d.arg < 1024 {
                    let packed = (a.arg << 20) | (b.arg << 10) | d.arg;
                    code.instructions[i] =
                        Instruction::new(Opcode::LoadFastLoadConstSubscrStoreFast, packed);
                    is_nop[i + 1] = true;
                    is_nop[i + 2] = true;
                    is_nop[i + 3] = true;
                    i += 4;
                    continue;
                }
            }
        }

        // LoadFast + LoadFast + BinarySubscr + StoreFast → LoadFastLoadFastSubscrStoreFast
        // Zero-Arc: reads container and key from locals by reference.
        // Encoding: (container_idx << 24) | (key_idx << 16) | (store_idx << 8)
        if i + 3 < n
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadFast
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
        {
            let c = &code.instructions[i + 2];
            if c.op == Opcode::BinarySubscr && !jump_targets[i + 3] && !is_nop[i + 3] {
                let d = &code.instructions[i + 3];
                if d.op == Opcode::StoreFast && a.arg < 256 && b.arg < 256 && d.arg < 256 {
                    let packed = (a.arg << 24) | (b.arg << 16) | (d.arg << 8);
                    code.instructions[i] =
                        Instruction::new(Opcode::LoadFastLoadFastSubscrStoreFast, packed);
                    is_nop[i + 1] = true;
                    is_nop[i + 2] = true;
                    is_nop[i + 3] = true;
                    i += 4;
                    continue;
                }
            }
        }

        // LoadFast + LoadFast + LoadFast + StoreSubscr → LoadFastLoadFastLoadFastStoreSubscr
        // Pattern: value = LOAD_FAST val_idx; container = LOAD_FAST cont_idx; key = LOAD_FAST key_idx; STORE_SUBSCR
        // Encoding: (val_idx << 24) | (container_idx << 16) | (key_idx << 8)
        if i + 3 < n
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadFast
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
        {
            let c = &code.instructions[i + 2];
            if c.op == Opcode::LoadFast && !jump_targets[i + 3] && !is_nop[i + 3] {
                let d = &code.instructions[i + 3];
                if d.op == Opcode::StoreSubscr && a.arg < 256 && b.arg < 256 && c.arg < 256 {
                    let packed = (a.arg << 24) | (b.arg << 16) | (c.arg << 8);
                    code.instructions[i] =
                        Instruction::new(Opcode::LoadFastLoadFastLoadFastStoreSubscr, packed);
                    is_nop[i + 1] = true;
                    is_nop[i + 2] = true;
                    is_nop[i + 3] = true;
                    i += 4;
                    continue;
                }
            }
        }

        // LoadFast + LoadFast + CompareOp(in/not_in) + StoreFast → LoadFastLoadFastContainsStoreFast
        // Zero-Arc: borrows needle/haystack from locals, stores bool in dest local.
        // Encoding: (needle_idx << 24) | (haystack_idx << 16) | (store_idx << 8) | in_flag
        if i + 3 < n
            && a.op == Opcode::LoadFast
            && b.op == Opcode::LoadFast
            && !jump_targets[i + 2]
            && !is_nop[i + 2]
        {
            let c = &code.instructions[i + 2];
            if c.op == Opcode::CompareOp
                && (c.arg == 6 || c.arg == 7)
                && !jump_targets[i + 3]
                && !is_nop[i + 3]
            {
                let d = &code.instructions[i + 3];
                if d.op == Opcode::StoreFast && a.arg < 256 && b.arg < 256 && d.arg < 256 {
                    let in_flag = if c.arg == 7 { 1u32 } else { 0u32 };
                    let packed = (a.arg << 24) | (b.arg << 16) | (d.arg << 8) | in_flag;
                    code.instructions[i] =
                        Instruction::new(Opcode::LoadFastLoadFastContainsStoreFast, packed);
                    is_nop[i + 1] = true;
                    is_nop[i + 2] = true;
                    is_nop[i + 3] = true;
                    i += 4;
                    continue;
                }
            }
        }

        // LoadFast + LoadAttr → LoadFastLoadAttr
        // Encoding: (local_idx << 16) | name_idx
        if a.op == Opcode::LoadFast
            && b.op == Opcode::LoadAttr
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            // Try 3-way: LoadFast + LoadAttr + StoreFast → LoadFastLoadAttrStoreFast
            if i + 2 < n
                && !jump_targets[i + 2]
                && !is_nop[i + 2]
                && code.instructions[i + 2].op == Opcode::StoreFast
                && a.arg < 1024
                && b.arg < 1024
                && code.instructions[i + 2].arg < 1024
            {
                let store_idx = code.instructions[i + 2].arg;
                let packed = (a.arg << 20) | (b.arg << 10) | store_idx;
                code.instructions[i] = Instruction::new(Opcode::LoadFastLoadAttrStoreFast, packed);
                is_nop[i + 1] = true;
                is_nop[i + 2] = true;
                i += 3;
                continue;
            }
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadAttr, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // StoreFast + JumpAbsolute → StoreFastJumpAbsolute (hot at loop body end)
        if a.op == Opcode::StoreFast
            && b.op == Opcode::JumpAbsolute
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::StoreFastJumpAbsolute, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // PopTop + JumpAbsolute → PopTopJumpAbsolute (void call loop end)
        if a.op == Opcode::PopTop && b.op == Opcode::JumpAbsolute {
            code.instructions[i] = Instruction::new(Opcode::PopTopJumpAbsolute, b.arg);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadFast + LoadMethod → LoadFastLoadMethod
        if a.op == Opcode::LoadFast
            && b.op == Opcode::LoadMethod
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadFastLoadMethod, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadFast + ReturnValue → LoadFastReturnValue (common function return)
        if a.op == Opcode::LoadFast && b.op == Opcode::ReturnValue {
            code.instructions[i] = Instruction::new(Opcode::LoadFastReturnValue, a.arg);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadConst + ReturnValue → LoadConstReturnValue (return literal)
        if a.op == Opcode::LoadConst && b.op == Opcode::ReturnValue {
            code.instructions[i] = Instruction::new(Opcode::LoadConstReturnValue, a.arg);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // LoadConst + StoreFast → LoadConstStoreFast (variable initialization)
        if a.op == Opcode::LoadConst
            && b.op == Opcode::StoreFast
            && a.arg <= 0xFFFF
            && b.arg <= 0xFFFF
        {
            let packed = (a.arg << 16) | b.arg;
            code.instructions[i] = Instruction::new(Opcode::LoadConstStoreFast, packed);
            is_nop[i + 1] = true;
            i += 2;
            continue;
        }

        // CallMethod + PopTop → CallMethodPopTop (discard return value)
        if a.op == Opcode::CallMethod && b.op == Opcode::PopTop {
            code.instructions[i] = Instruction::new(Opcode::CallMethodPopTop, a.arg);
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
        if !nop {
            new_idx += 1;
        }
    }
    // Sentinel for targets pointing past the end
    let final_len = new_idx;

    if final_len == n {
        return;
    } // nothing was fused

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
        } else if instr.op == Opcode::LoadFastCompareConstJump {
            // Jump target is in low 12 bits; other fields in upper bits
            let upper = instr.arg & 0xFFFFF000;
            let target = (instr.arg & 0xFFF) as usize;
            let new_target = if target < old_to_new.len() {
                old_to_new[target] as u32
            } else {
                final_len as u32
            };
            instr.arg = upper | (new_target & 0xFFF);
        } else if instr.op == Opcode::ForIterStoreFast {
            // Jump target is in high 16 bits; store_idx in low 16 bits
            let jump_target = (instr.arg >> 16) as usize;
            let store_idx = instr.arg & 0xFFFF;
            let new_target = if jump_target < old_to_new.len() {
                old_to_new[jump_target] as u32
            } else {
                final_len as u32
            };
            instr.arg = (new_target << 16) | store_idx;
        } else if instr.op == Opcode::StoreFastJumpAbsolute {
            // store_idx in high 16 bits; jump_target in low 16 bits
            let store_idx = instr.arg >> 16;
            let jump_target = (instr.arg & 0xFFFF) as usize;
            let new_target = if jump_target < old_to_new.len() {
                old_to_new[jump_target] as u32
            } else {
                final_len as u32
            };
            instr.arg = (store_idx << 16) | new_target;
        } else if instr.op == Opcode::LoadFastLoadFastCompareJump {
            // Jump target is in low 12 bits (same layout as LoadFastCompareConstJump)
            let upper = instr.arg & 0xFFFFF000;
            let target = (instr.arg & 0xFFF) as usize;
            let new_target = if target < old_to_new.len() {
                old_to_new[target] as u32
            } else {
                final_len as u32
            };
            instr.arg = upper | (new_target & 0xFFF);
        } else if instr.op.is_jump() {
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
