//! Fallback helpers for fused compare-and-jump opcodes.

use crate::VirtualMachine;
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::PyResult;
use ferrython_core::object::PyObjectPayload;
use ferrython_core::types::PyInt;

impl VirtualMachine {
    pub(crate) fn fallback_compare_jump(
        &mut self,
        cmp_op: u32,
        jump_target: usize,
    ) -> PyResult<()> {
        let result = self.exec_compare_ops(Instruction::new(Opcode::CompareOp, cmp_op))?;
        if result.is_none() {
            let frame = self.call_stack.last_mut().expect("missing frame");
            let value = frame.stack.pop().expect("stack underflow");
            let is_false = if cmp_op == 10 {
                matches!(&value.payload, PyObjectPayload::Bool(false))
            } else {
                match &value.payload {
                    PyObjectPayload::Bool(value) => !value,
                    PyObjectPayload::None => true,
                    PyObjectPayload::Int(PyInt::Small(value)) => *value == 0,
                    _ => !self.vm_is_truthy(&value)?,
                }
            };
            if is_false {
                let frame = self.call_stack.last_mut().expect("missing frame");
                frame.ip = jump_target;
            }
        }
        Ok(())
    }
}
