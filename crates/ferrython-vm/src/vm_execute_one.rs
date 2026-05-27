//! Cold fallback opcode dispatch used by the VM main loop.

use crate::VirtualMachine;
use ferrython_core::error::PyException;
use ferrython_core::object::PyObjectRef;

impl VirtualMachine {
    #[cold]
    #[inline(never)]
    pub(crate) fn execute_one(
        &mut self,
        instr: ferrython_bytecode::Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        use ferrython_bytecode::opcode::Opcode;
        match instr.op {
            Opcode::Nop
            | Opcode::PopTop
            | Opcode::PopTopJumpAbsolute
            | Opcode::RotTwo
            | Opcode::RotThree
            | Opcode::RotFour
            | Opcode::DupTop
            | Opcode::DupTopTwo
            | Opcode::LoadConst => self.exec_stack_ops(instr),

            Opcode::LoadName
            | Opcode::StoreName
            | Opcode::DeleteName
            | Opcode::LoadFast
            | Opcode::StoreFast
            | Opcode::DeleteFast
            | Opcode::LoadDeref
            | Opcode::StoreDeref
            | Opcode::DeleteDeref
            | Opcode::LoadClosure
            | Opcode::LoadClassderef
            | Opcode::LoadGlobal
            | Opcode::StoreGlobal
            | Opcode::DeleteGlobal
            | Opcode::LoadFastLoadFast
            | Opcode::LoadFastLoadConst
            | Opcode::StoreFastLoadFast
            | Opcode::StoreFastJumpAbsolute
            | Opcode::LoadConstStoreFast
            | Opcode::LoadGlobalStoreFast
            | Opcode::LoadConstLoadFastContainsStoreFast
            | Opcode::LoadFastLoadConstSubscrStoreFast
            | Opcode::LoadFastLoadFastSubscrStoreFast
            | Opcode::LoadFastLoadFastLoadFastStoreSubscr
            | Opcode::LoadFastLoadFastContainsStoreFast => self.exec_name_ops(instr),

            Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr => self.exec_attr_ops(instr),

            Opcode::UnaryPositive
            | Opcode::UnaryNegative
            | Opcode::UnaryNot
            | Opcode::UnaryInvert => self.exec_unary_ops(instr),

            Opcode::BinaryAdd
            | Opcode::InplaceAdd
            | Opcode::BinarySubtract
            | Opcode::InplaceSubtract
            | Opcode::BinaryMultiply
            | Opcode::InplaceMultiply
            | Opcode::BinaryTrueDivide
            | Opcode::InplaceTrueDivide
            | Opcode::BinaryFloorDivide
            | Opcode::InplaceFloorDivide
            | Opcode::BinaryModulo
            | Opcode::InplaceModulo
            | Opcode::BinaryPower
            | Opcode::InplacePower
            | Opcode::BinaryLshift
            | Opcode::InplaceLshift
            | Opcode::BinaryRshift
            | Opcode::InplaceRshift
            | Opcode::BinaryAnd
            | Opcode::InplaceAnd
            | Opcode::BinaryOr
            | Opcode::InplaceOr
            | Opcode::BinaryXor
            | Opcode::InplaceXor
            | Opcode::BinaryMatrixMultiply
            | Opcode::InplaceMatrixMultiply
            | Opcode::LoadFastLoadConstBinarySub
            | Opcode::LoadFastLoadConstBinaryAdd
            | Opcode::LoadFastLoadFastBinaryAdd
            | Opcode::LoadFastLoadFastBinaryAddStoreFast
            | Opcode::LoadFastLoadConstBinaryAddStoreFast => self.exec_binary_ops(instr),

            Opcode::BinarySubscr | Opcode::StoreSubscr | Opcode::DeleteSubscr => {
                self.exec_subscript_ops(instr)
            }

            Opcode::CompareOp
            | Opcode::CompareOpPopJumpIfFalse
            | Opcode::LoadFastCompareConstJump
            | Opcode::LoadFastLoadFastCompareJump => self.exec_compare_ops(instr),

            Opcode::JumpForward
            | Opcode::JumpAbsolute
            | Opcode::JumpFinally
            | Opcode::PopJumpIfFalse
            | Opcode::PopJumpIfTrue
            | Opcode::JumpIfTrueOrPop
            | Opcode::JumpIfFalseOrPop
            | Opcode::GetIter
            | Opcode::GetYieldFromIter
            | Opcode::ForIter
            | Opcode::ForIterStoreFast
            | Opcode::EndForLoop
            | Opcode::PopBlockJump => self.exec_jump_ops(instr),

            Opcode::BuildTuple
            | Opcode::BuildList
            | Opcode::BuildSet
            | Opcode::BuildMap
            | Opcode::BuildConstKeyMap
            | Opcode::BuildString
            | Opcode::ListAppend
            | Opcode::SetAdd
            | Opcode::MapAdd
            | Opcode::DictUpdate
            | Opcode::DictMerge
            | Opcode::ListExtend
            | Opcode::SetUpdate
            | Opcode::ListToTuple
            | Opcode::BuildSlice
            | Opcode::UnpackSequence
            | Opcode::UnpackEx => self.exec_build_ops(instr),

            Opcode::CallFunction
            | Opcode::CallFunctionKw
            | Opcode::CallMethod
            | Opcode::CallMethodPopTop
            | Opcode::CallFunctionEx
            | Opcode::LoadMethod
            | Opcode::MakeFunction
            | Opcode::LoadGlobalCallFunction
            | Opcode::LoadFastLoadAttr
            | Opcode::LoadFastLoadMethod => self.exec_call_ops(instr),

            Opcode::ReturnValue
            | Opcode::LoadFastReturnValue
            | Opcode::LoadConstReturnValue
            | Opcode::ImportName
            | Opcode::ImportFrom
            | Opcode::ImportStar => self.exec_return_import(instr),

            Opcode::SetupFinally
            | Opcode::SetupExcept
            | Opcode::PopBlock
            | Opcode::PopExcept
            | Opcode::EndFinally
            | Opcode::BeginFinally
            | Opcode::CancelFinally
            | Opcode::RaiseVarargs
            | Opcode::SetupWith
            | Opcode::SetupAsyncWith
            | Opcode::WithCleanupStart
            | Opcode::WithCleanupFinish => self.exec_exception_ops(instr),

            Opcode::PrintExpr
            | Opcode::LoadBuildClass
            | Opcode::SetupAnnotations
            | Opcode::FormatValue
            | Opcode::ExtendedArg
            | Opcode::YieldValue
            | Opcode::YieldFrom
            | Opcode::GetAwaitable
            | Opcode::GetAiter
            | Opcode::GetAnext
            | Opcode::BeforeAsyncWith
            | Opcode::EndAsyncFor => self.exec_misc_ops(instr),

            #[allow(unreachable_patterns)]
            _ => Err(PyException::runtime_error(format!(
                "unimplemented opcode: {:?}",
                instr.op
            ))),
        }
    }
}
