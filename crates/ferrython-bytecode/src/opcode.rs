//! Python 3.8 opcode definitions.
//!
//! Each opcode matches CPython 3.8's opcode table. Instructions use
//! 2-byte wordcode format: 1 byte opcode + 1 byte arg (with EXTENDED_ARG
//! for arguments > 255).

/// A single bytecode instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub op: Opcode,
    pub arg: u32,
}

impl Instruction {
    pub fn new(op: Opcode, arg: u32) -> Self {
        Self { op, arg }
    }

    pub fn simple(op: Opcode) -> Self {
        Self { op, arg: 0 }
    }
}

/// Python 3.8 opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Opcode {
    // ── Stack operations ──
    PopTop = 1,
    RotTwo = 2,
    RotThree = 3,
    DupTop = 4,
    DupTopTwo = 5,
    RotFour = 6,
    Nop = 9,

    // ── Unary operations ──
    UnaryPositive = 10,
    UnaryNegative = 11,
    UnaryNot = 12,
    UnaryInvert = 15,

    // ── Binary operations ──
    BinaryMatrixMultiply = 16,
    InplaceMatrixMultiply = 17,
    BinaryPower = 19,
    BinaryMultiply = 20,
    BinaryModulo = 22,
    BinaryAdd = 23,
    BinarySubtract = 24,
    BinarySubscr = 25,
    BinaryFloorDivide = 26,
    BinaryTrueDivide = 27,
    InplaceFloorDivide = 28,
    InplaceTrueDivide = 29,

    // ── Misc ──
    GetAiter = 50,
    GetAnext = 51,
    BeforeAsyncWith = 52,
    BeginFinally = 53,
    EndAsyncFor = 54,

    InplaceAdd = 55,
    InplaceSubtract = 56,
    InplaceMultiply = 57,
    InplaceModulo = 59,
    StoreSubscr = 60,
    DeleteSubscr = 61,
    BinaryLshift = 62,
    BinaryRshift = 63,
    BinaryAnd = 64,
    BinaryXor = 65,
    BinaryOr = 66,
    InplacePower = 67,
    GetIter = 68,
    GetYieldFromIter = 69,
    PrintExpr = 70,
    LoadBuildClass = 71,
    YieldFrom = 72,
    GetAwaitable = 73,

    InplaceLshift = 75,
    InplaceRshift = 76,
    InplaceAnd = 77,
    InplaceXor = 78,
    InplaceOr = 79,

    // ── Exception handling (no arg) ──
    WithCleanupStart = 81,
    WithCleanupFinish = 82,
    ReturnValue = 83,
    ImportStar = 84,
    SetupAnnotations = 85,
    YieldValue = 86,
    PopBlock = 87,
    EndFinally = 88,
    PopExcept = 89,

    // ── Opcodes with arguments (>= 90) ──
    StoreName = 90,
    DeleteName = 91,
    UnpackSequence = 92,
    ForIter = 93,
    UnpackEx = 94,
    StoreAttr = 95,
    DeleteAttr = 96,
    StoreGlobal = 97,
    DeleteGlobal = 98,
    LoadConst = 100,
    LoadName = 101,
    BuildTuple = 102,
    BuildList = 103,
    BuildSet = 104,
    BuildMap = 105,
    LoadAttr = 106,
    CompareOp = 107,
    ImportName = 108,
    ImportFrom = 109,
    JumpForward = 110,
    JumpIfFalseOrPop = 111,
    JumpIfTrueOrPop = 112,
    JumpAbsolute = 113,
    PopJumpIfFalse = 114,
    PopJumpIfTrue = 115,
    LoadGlobal = 116,
    SetupFinally = 122,
    LoadFast = 124,
    StoreFast = 125,
    DeleteFast = 126,
    RaiseVarargs = 130,
    CallFunction = 131,
    MakeFunction = 132,
    BuildSlice = 133,
    LoadClosure = 135,
    LoadDeref = 136,
    StoreDeref = 137,
    DeleteDeref = 138,
    CallFunctionKw = 141,
    CallFunctionEx = 142,
    SetupWith = 143,
    ListAppend = 145,
    SetAdd = 146,
    MapAdd = 147,
    LoadClassderef = 148,
    ExtendedArg = 144,
    BuildConstKeyMap = 156,
    BuildString = 157,

    // ── Format ──
    FormatValue = 155,

    // ── List/Set/Dict extend ops ──
    ListExtend = 162,
    SetUpdate = 163,
    DictMerge = 164,
    DictUpdate = 165,
    ListToTuple = 166,

    // ── Method call optimization ──
    LoadMethod = 160,
    CallMethod = 161,

    // ── Custom: distinguish except from finally ──
    SetupExcept = 200,
    SetupAsyncWith = 201,
    /// Pop iterator from stack and call close() if it's a generator.
    /// Used by `break` in for-loops to ensure generator finally blocks run.
    EndForLoop = 202,

    // ── Superinstructions (peephole fused pairs) ──
    /// Two consecutive LoadFast. arg = (idx1 << 16) | idx2.
    LoadFastLoadFast = 210,
    /// LoadFast then LoadConst. arg = (fast_idx << 16) | const_idx.
    LoadFastLoadConst = 211,
    /// StoreFast then LoadFast. arg = (store_idx << 16) | load_idx.
    StoreFastLoadFast = 212,
    /// CompareOp then PopJumpIfFalse. arg = (cmp_op << 24) | jump_target.
    /// Fast-paths int/float comparisons without intermediate bool push/pop.
    CompareOpPopJumpIfFalse = 213,
    /// LoadFast + LoadConst + BinarySubtract fused.
    /// arg = (fast_idx << 16) | const_idx.
    /// Fast-paths int-int subtraction without intermediate stack pushes.
    LoadFastLoadConstBinarySub = 214,
    /// LoadFast + LoadConst + BinaryAdd fused.
    /// arg = (fast_idx << 16) | const_idx.
    /// Fast-paths int-int addition without intermediate stack pushes.
    LoadFastLoadConstBinaryAdd = 215,
    /// ForIter + StoreFast fused.
    /// arg encoding: (jump_target << 16) | store_idx.
    /// Avoids intermediate stack push/pop in `for i in range(n)` loops.
    ForIterStoreFast = 216,
    /// LoadGlobal + CallFunction fused.
    /// arg encoding: (name_idx << 16) | arg_count.
    /// Avoids intermediate stack push of the function object.
    LoadGlobalCallFunction = 217,

    /// Fused LoadFast + LoadAttr — load local variable then read attribute.
    /// arg encoding: (local_idx << 16) | name_idx.
    LoadFastLoadAttr = 218,
    /// Fused LoadFast + LoadFast + BinaryAdd (a + b where both are locals)
    LoadFastLoadFastBinaryAdd = 219,
    /// Fused LoadFastLoadConst + CompareOpPopJumpIfFalse — 4-way zero-clone.
    /// Reads local and constant by reference, compares, jumps if false.
    /// arg encoding: (cmp_op << 28) | (local_idx << 20) | (const_idx << 12) | jump_target
    /// Limits: local_idx < 256, const_idx < 256, jump_target < 4096, cmp_op < 16
    LoadFastCompareConstJump = 220,
    /// Fused StoreFast + JumpAbsolute — hot at end of loop bodies.
    /// arg encoding: (store_idx << 16) | jump_target
    StoreFastJumpAbsolute = 221,
    /// Fused PopTop + JumpAbsolute — hot at end of loop bodies with void calls.
    /// arg = jump_target
    PopTopJumpAbsolute = 222,
    /// Fused LoadFast + LoadMethod — common in method call patterns.
    /// arg encoding: (local_idx << 16) | name_idx
    LoadFastLoadMethod = 223,
    /// Fused LoadFast + LoadFast + BinaryAdd + StoreFast — hot accumulator pattern (x = x + i).
    /// arg encoding: (src1 << 16) | (src2 << 8) | dest
    LoadFastLoadFastBinaryAddStoreFast = 224,
    /// Fused LoadFast + LoadConst + BinaryAdd + StoreFast — hot constant-add pattern (x = x + 1.0).
    /// arg encoding: (local_idx << 16) | (const_idx << 8) | dest
    LoadFastLoadConstBinaryAddStoreFast = 225,
    /// Fused LoadFast + ReturnValue — common function return pattern (`return x`).
    /// arg = local_idx
    LoadFastReturnValue = 226,
    /// Fused LoadConst + ReturnValue — common literal return (`return 0`, `return None`).
    /// arg = const_idx
    LoadConstReturnValue = 227,
    /// Fused LoadConst + StoreFast — common variable initialization (`x = 0`, `x = None`).
    /// arg encoding: (const_idx << 16) | store_idx
    LoadConstStoreFast = 228,
    /// Fused CallMethod + PopTop — common for methods whose return value is discarded.
    /// arg = arg_count (same as CallMethod)
    CallMethodPopTop = 229,
    /// Fused LoadFast + LoadAttr + StoreFast — common `x = obj.attr` pattern.
    /// arg encoding: (local_idx << 20) | (name_idx << 10) | store_idx
    /// Limits: local_idx < 1024, name_idx < 1024, store_idx < 1024
    LoadFastLoadAttrStoreFast = 230,

    /// Fused LoadFast + LoadConst + BinaryMultiply — hot in `x * c` patterns.
    /// arg encoding: (fast_idx << 16) | const_idx
    LoadFastLoadConstBinaryMul = 231,
    /// Fused LoadFast + LoadConst + BinaryMultiply + StoreFast — hot `x = x * c` pattern.
    /// arg encoding: (local_idx << 16) | (const_idx << 8) | dest
    LoadFastLoadConstBinaryMulStoreFast = 232,
    /// Fused LoadFast + LoadConst + BinarySub + StoreFast — hot `x = x - 1` pattern.
    /// arg encoding: (local_idx << 16) | (const_idx << 8) | dest
    LoadFastLoadConstBinarySubStoreFast = 233,
    /// Fused 6-way: LoadFast + LoadConst + BinaryMul + LoadConst + BinaryMod + StoreFast
    /// Hot pattern: `x = (x * c1) % c2`
    /// arg encoding: stored in a u64 but packed as two u32s via arg+arg2:
    ///   arg = (local_idx << 24) | (const1_idx << 16) | (const2_idx << 8) | dest
    /// Limits: all indices < 256
    LoadFastMulModStoreFast = 234,
    /// Fused 4-way: LoadFast + LoadFast + CompareOp + PopJumpIfFalse
    /// Zero-clone: reads both locals by reference, no stack ops.
    /// arg encoding: (cmp_op << 28) | (idx1 << 20) | (idx2 << 12) | jump_target
    LoadFastLoadFastCompareJump = 235,
    /// Fused 2-way: LoadGlobal + StoreFast
    /// Stores global cache value directly to local, no stack push/pop.
    /// arg encoding: (name_idx << 16) | store_idx
    LoadGlobalStoreFast = 236,
    /// Fused 2-way: PopBlock + JumpForward/JumpAbsolute
    /// Hot in try/except — pops exception block and jumps in one dispatch.
    /// arg encoding: jump_target
    PopBlockJump = 237,
    /// Fused 4-way: LoadConst + LoadFast + CompareOp(in/not_in) + StoreFast
    /// Zero-clone: reads constant and local by reference, stores bool result in-place.
    /// arg encoding: (not_in_flag << 31) | (const_idx << 20) | (fast_idx << 10) | store_idx
    LoadConstLoadFastContainsStoreFast = 238,
    /// Fused 4-way: LoadFast + LoadConst + BinarySubscr + StoreFast
    /// Zero-clone for container and index; only clones element (with in-place mutation fallback).
    /// arg encoding: (fast_idx << 20) | (const_idx << 10) | store_idx
    LoadFastLoadConstSubscrStoreFast = 239,
    /// Fused 4-way: LoadFast + LoadFast + BinarySubscr + StoreFast
    /// Zero-clone for container and key from locals; only clones element.
    /// arg encoding: (container_idx << 24) | (key_idx << 16) | (store_idx << 8)
    LoadFastLoadFastSubscrStoreFast = 240,
    /// Fused 3-way: LoadFast + LoadFast + LoadFast + StoreSubscr
    /// Zero-Arc for container read; stores value directly.
    /// arg encoding: (val_idx << 24) | (container_idx << 16) | (key_idx << 8)
    LoadFastLoadFastLoadFastStoreSubscr = 241,
    /// Fused LOAD_FAST + LOAD_FAST + COMPARE_OP(in/not_in) + STORE_FAST.
    /// Zero-Arc containment check: borrows needle/haystack from locals, stores bool result.
    /// arg encoding: (needle_idx << 24) | (haystack_idx << 16) | (store_idx << 8) | in_flag
    LoadFastLoadFastContainsStoreFast = 242,
}

impl Opcode {
    /// Returns true if this opcode takes an argument.
    pub fn has_arg(self) -> bool {
        (self as u8) >= 90
    }

    /// Returns the stack effect of this opcode.
    /// Positive = pushes, negative = pops.
    /// Some opcodes have variable effects depending on arg.
    pub fn stack_effect(self, arg: u32) -> i32 {
        match self {
            Self::PopTop => -1,
            Self::RotTwo | Self::RotThree | Self::RotFour => 0,
            Self::DupTop => 1,
            Self::DupTopTwo => 2,
            Self::Nop => 0,
            Self::UnaryPositive | Self::UnaryNegative | Self::UnaryNot | Self::UnaryInvert => 0,
            Self::BinaryPower
            | Self::BinaryMultiply
            | Self::BinaryMatrixMultiply
            | Self::BinaryModulo
            | Self::BinaryAdd
            | Self::BinarySubtract
            | Self::BinaryFloorDivide
            | Self::BinaryTrueDivide
            | Self::BinaryLshift
            | Self::BinaryRshift
            | Self::BinaryAnd
            | Self::BinaryXor
            | Self::BinaryOr
            | Self::BinarySubscr => -1,
            Self::InplaceAdd
            | Self::InplaceSubtract
            | Self::InplaceMultiply
            | Self::InplaceModulo
            | Self::InplacePower
            | Self::InplaceFloorDivide
            | Self::InplaceTrueDivide
            | Self::InplaceMatrixMultiply
            | Self::InplaceLshift
            | Self::InplaceRshift
            | Self::InplaceAnd
            | Self::InplaceXor
            | Self::InplaceOr => -1,
            Self::StoreSubscr => -3,
            Self::DeleteSubscr => -2,
            Self::GetIter => 0,
            Self::PrintExpr => -1,
            Self::ReturnValue => -1,
            Self::YieldValue => 0,
            Self::YieldFrom => -1,
            Self::LoadBuildClass => 1,
            Self::LoadConst => 1,
            Self::LoadName | Self::LoadGlobal | Self::LoadFast | Self::LoadDeref
            | Self::LoadClassderef | Self::LoadClosure => 1,
            Self::StoreName | Self::StoreGlobal | Self::StoreFast | Self::StoreDeref => -1,
            Self::DeleteName | Self::DeleteGlobal | Self::DeleteFast | Self::DeleteDeref => 0,
            Self::LoadAttr => 0,
            // LoadMethod pops the object and pushes 2 items (method + receiver), net +1
            Self::LoadMethod => 1,
            Self::StoreAttr => -2,
            Self::DeleteAttr => -1,
            Self::BuildTuple | Self::BuildList | Self::BuildSet => -(arg as i32) + 1,
            Self::BuildMap => -(2 * arg as i32) + 1,
            Self::BuildConstKeyMap => -(arg as i32),
            Self::BuildString => -(arg as i32) + 1,
            Self::BuildSlice => if arg == 3 { -2 } else { -1 },
            Self::CompareOp => -1,
            Self::JumpForward | Self::JumpAbsolute => 0,
            Self::PopJumpIfFalse | Self::PopJumpIfTrue => -1,
            Self::JumpIfFalseOrPop | Self::JumpIfTrueOrPop => 0, // varies
            Self::ForIter => 1, // pushes next or jumps
            Self::UnpackSequence => arg as i32 - 1,
            Self::UnpackEx => (arg as i32 & 0xFF) + (arg as i32 >> 8),
            Self::CallFunction => -(arg as i32),
            Self::CallFunctionKw => -(arg as i32) - 1,
            Self::CallFunctionEx => if arg & 1 != 0 { -3 } else { -2 },
            Self::CallMethod => -(arg as i32) - 1,
            Self::MakeFunction => {
                let mut effect: i32 = -1; // qualname
                if arg & 0x01 != 0 { effect -= 1; } // defaults
                if arg & 0x02 != 0 { effect -= 1; } // kwdefaults
                if arg & 0x04 != 0 { effect -= 1; } // annotations
                if arg & 0x08 != 0 { effect -= 1; } // closure
                effect
            }
            Self::ImportName => -1,
            Self::ImportFrom => 1,
            Self::ImportStar => -1,
            Self::SetupFinally | Self::SetupWith | Self::SetupExcept
            | Self::SetupAsyncWith => 0,
            Self::EndForLoop => -1,
            Self::PopBlock | Self::PopExcept => 0,
            Self::EndFinally | Self::BeginFinally => 0,
            Self::RaiseVarargs => -(arg as i32),
            Self::ListAppend | Self::SetAdd | Self::MapAdd => -1,
            Self::ListExtend | Self::SetUpdate | Self::DictMerge | Self::DictUpdate => -1,
            Self::ListToTuple => 0, // pops list, pushes tuple
            Self::FormatValue => if arg & 0x04 != 0 { -1 } else { 0 },
            Self::GetAwaitable | Self::GetAiter | Self::GetAnext
            | Self::GetYieldFromIter => 0,
            Self::BeforeAsyncWith => 1,
            Self::EndAsyncFor => -7,
            Self::WithCleanupStart => 1,
            Self::WithCleanupFinish => -1,
            Self::SetupAnnotations => 0,
            Self::ExtendedArg => 0,
            Self::LoadFastLoadFast => 2,
            Self::LoadFastLoadConst => 2,
            Self::StoreFastLoadFast => 0, // -1 (store) + 1 (load) = 0
            Self::CompareOpPopJumpIfFalse => -2, // pops 2 operands, pushes nothing
            Self::LoadFastLoadConstBinarySub => 1, // loads local + const, subtracts, pushes result
            Self::LoadFastLoadConstBinaryAdd => 1, // loads local + const, adds, pushes result
            Self::LoadFastLoadFastBinaryAdd => 1, // loads two locals, adds, pushes result
            Self::ForIterStoreFast => 0, // either jumps (pops iter) or stores to local (net 0)
            Self::LoadGlobalCallFunction => {
                // Pops arg_count args from stack, pushes result (function never on stack)
                let arg_count = (arg & 0xFFFF) as i32;
                -arg_count + 1
            }
            Self::LoadFastLoadAttr => 1, // push local, replace TOS with attr → net +1
            Self::LoadFastCompareConstJump => 0, // reads local+const by ref, compares, no stack change
            Self::StoreFastJumpAbsolute => -1, // pops TOS and stores to local, then jumps
            Self::PopTopJumpAbsolute => -1, // pops TOS, then jumps
            Self::LoadFastLoadMethod => 2, // pushes [slot_0, slot_1] like LoadMethod
            Self::LoadFastLoadFastBinaryAddStoreFast => 0,
            Self::LoadFastLoadConstBinaryAddStoreFast => 0,
            Self::LoadFastReturnValue => 0, // reads local, returns it (no net stack change)
            Self::LoadConstReturnValue => 0, // reads const, returns it
            Self::LoadConstStoreFast => 0, // reads const, stores to local (net 0)
            Self::CallMethodPopTop => {
                // Pops: method + receiver + arg_count args. Pushes nothing (result discarded).
                -(arg as i32) - 2
            }
            Self::LoadFastLoadAttrStoreFast => 0, // reads local, gets attr, stores to local (net 0)
            Self::LoadFastLoadConstBinaryMul => 1, // pushes result
            Self::LoadFastLoadConstBinaryMulStoreFast => 0, // stores to local
            Self::LoadFastLoadConstBinarySubStoreFast => 0, // stores to local
            Self::LoadFastMulModStoreFast => 0, // x = (x * c1) % c2
            Self::LoadFastLoadFastCompareJump => 0, // reads two locals by ref, compares, no stack change
            Self::LoadGlobalStoreFast => 0, // stores global directly to local (net 0)
            Self::PopBlockJump => 0, // pops block, jumps (no stack change)
            Self::LoadConstLoadFastContainsStoreFast => 0, // reads const+local by ref, stores bool to local
            Self::LoadFastLoadConstSubscrStoreFast => 0, // reads local+const by ref, stores element to local
            Self::LoadFastLoadFastSubscrStoreFast => 0,  // reads 2 locals by ref, stores element to local
            Self::LoadFastLoadFastLoadFastStoreSubscr => -3, // reads 3 locals, stores to container (net: 0 but pops 3 pushes)
            Self::LoadFastLoadFastContainsStoreFast => 0, // borrows 2 locals, stores bool in local
        }
    }

    /// Returns true if this opcode is a jump instruction.
    pub fn is_jump(self) -> bool {
        matches!(
            self,
            Self::JumpForward
                | Self::JumpAbsolute
                | Self::PopJumpIfFalse
                | Self::PopJumpIfTrue
                | Self::JumpIfFalseOrPop
                | Self::JumpIfTrueOrPop
                | Self::ForIter
                | Self::SetupFinally
                | Self::SetupWith
                | Self::SetupExcept
                | Self::SetupAsyncWith
                | Self::CompareOpPopJumpIfFalse
                | Self::ForIterStoreFast
                | Self::LoadFastCompareConstJump
                | Self::StoreFastJumpAbsolute
                | Self::PopTopJumpAbsolute
                | Self::LoadFastLoadFastCompareJump
                | Self::PopBlockJump
        )
    }
}
