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
            Self::LoadAttr | Self::LoadMethod => 0,
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
            Self::ForIterStoreFast => 0, // either jumps (pops iter) or stores to local (net 0)
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
        )
    }
}
