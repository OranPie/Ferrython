//! Fallback decomposition for fused VM superinstructions.

use crate::VirtualMachine;
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::PyResult;

impl VirtualMachine {
    pub(crate) fn fallback_fused_collection(&mut self, instr: Instruction) -> PyResult<()> {
        match instr.op {
            Opcode::LoadConstLoadFastContainsStoreFast => {
                self.fallback_const_fast_contains_store(instr)
            }
            Opcode::LoadFastLoadConstSubscrStoreFast => {
                self.fallback_fast_const_subscr_store(instr)
            }
            Opcode::LoadFastLoadFastSubscrStoreFast => self.fallback_fast_fast_subscr_store(instr),
            Opcode::LoadFastLoadFastLoadFastStoreSubscr => {
                self.fallback_fast_fast_store_subscr(instr)
            }
            Opcode::LoadFastLoadFastContainsStoreFast => {
                self.fallback_fast_fast_contains_store(instr)
            }
            _ => Ok(()),
        }
    }

    fn fallback_const_fast_contains_store(&mut self, instr: Instruction) -> PyResult<()> {
        let not_in = (instr.arg >> 31) != 0;
        let const_idx = ((instr.arg >> 20) & 0x3FF) as usize;
        let fast_idx = ((instr.arg >> 10) & 0x3FF) as usize;
        let store_idx = (instr.arg & 0x3FF) as usize;

        {
            let frame = self.call_stack.last_mut().expect("missing frame");
            frame.push(unsafe { frame.constant_cache.get_unchecked(const_idx).clone() });
            if let Some(value) = frame.locals.get(fast_idx).and_then(|v| v.as_ref()).cloned() {
                frame.push(value);
            } else {
                let _ = frame.pop();
                return Self::err_unbound_local(&frame.code.varnames, fast_idx).map(drop);
            }
        }

        let cmp_arg = if not_in { 7u32 } else { 6u32 };
        self.execute_one(Instruction::new(Opcode::CompareOp, cmp_arg))?;
        let frame = self.call_stack.last_mut().expect("missing frame");
        let value = frame.pop();
        unsafe { frame.set_local_unchecked(store_idx, value) };
        Ok(())
    }

    fn fallback_fast_const_subscr_store(&mut self, instr: Instruction) -> PyResult<()> {
        let fast_idx = ((instr.arg >> 20) & 0x3FF) as usize;
        let const_idx = ((instr.arg >> 10) & 0x3FF) as usize;
        let store_idx = (instr.arg & 0x3FF) as usize;

        {
            let frame = self.call_stack.last_mut().expect("missing frame");
            if let Some(value) = frame.locals.get(fast_idx).and_then(|v| v.as_ref()).cloned() {
                frame.push(value);
            } else {
                return Self::err_unbound_local(&frame.code.varnames, fast_idx).map(drop);
            }
            frame.push(unsafe { frame.constant_cache.get_unchecked(const_idx).clone() });
        }

        self.execute_one(Instruction::new(Opcode::BinarySubscr, 0))?;
        let frame = self.call_stack.last_mut().expect("missing frame");
        let value = frame.pop();
        unsafe { frame.set_local_unchecked(store_idx, value) };
        Ok(())
    }

    fn fallback_fast_fast_subscr_store(&mut self, instr: Instruction) -> PyResult<()> {
        let container_idx = (instr.arg >> 24) as usize;
        let key_idx = ((instr.arg >> 16) & 0xFF) as usize;
        let store_idx = ((instr.arg >> 8) & 0xFF) as usize;

        self.push_local_or_unbound(container_idx)?;
        self.push_local_or_unbound(key_idx)?;
        self.execute_one(Instruction::new(Opcode::BinarySubscr, 0))?;
        let frame = self.call_stack.last_mut().expect("missing frame");
        let value = frame.pop();
        unsafe { frame.set_local_unchecked(store_idx, value) };
        Ok(())
    }

    fn fallback_fast_fast_store_subscr(&mut self, instr: Instruction) -> PyResult<()> {
        let val_idx = (instr.arg >> 24) as usize;
        let container_idx = ((instr.arg >> 16) & 0xFF) as usize;
        let key_idx = ((instr.arg >> 8) & 0xFF) as usize;

        self.push_local_or_unbound(val_idx)?;
        self.push_local_or_unbound(container_idx)?;
        self.push_local_or_unbound(key_idx)?;
        self.execute_one(Instruction::new(Opcode::StoreSubscr, 0))?;
        Ok(())
    }

    fn fallback_fast_fast_contains_store(&mut self, instr: Instruction) -> PyResult<()> {
        let needle_idx = (instr.arg >> 24) as usize;
        let haystack_idx = ((instr.arg >> 16) & 0xFF) as usize;
        let store_idx = ((instr.arg >> 8) & 0xFF) as usize;
        let negate = (instr.arg & 1) != 0;

        self.push_local_or_unbound(needle_idx)?;
        self.push_local_or_unbound(haystack_idx)?;
        let cmp_arg = if negate { 7u32 } else { 6u32 };
        self.execute_one(Instruction::new(Opcode::CompareOp, cmp_arg))?;
        let frame = self.call_stack.last_mut().expect("missing frame");
        let value = frame.pop();
        unsafe { frame.set_local_unchecked(store_idx, value) };
        Ok(())
    }

    fn push_local_or_unbound(&mut self, idx: usize) -> PyResult<()> {
        let frame = self.call_stack.last_mut().expect("missing frame");
        let Some(value) = frame.locals.get(idx).and_then(|v| v.as_ref()).cloned() else {
            return Self::err_unbound_local(&frame.code.varnames, idx).map(drop);
        };
        frame.push(value);
        Ok(())
    }
}
