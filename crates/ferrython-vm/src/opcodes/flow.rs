//! Control flow: jumps and iterator advancement.

mod build;
mod call;
mod return_import;

use crate::builtins;
use crate::frame::BlockKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

impl VirtualMachine {
    pub(crate) fn exec_jump_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::JumpForward | Opcode::JumpAbsolute => {
                self.vm_frame().ip = instr.arg as usize;
            }
            Opcode::JumpFinally => {
                let target = instr.arg as usize;
                let frame = self.vm_frame();
                let mut found_finally = false;
                while let Some(block) = frame.block_stack.last().copied() {
                    if block.kind() == BlockKind::Finally {
                        frame.block_stack.pop();
                        frame.pending_jump = Some(target);
                        frame.push(PyObject::none());
                        frame.ip = block.handler();
                        found_finally = true;
                        break;
                    } else {
                        frame.block_stack.pop();
                    }
                }
                if !found_finally {
                    frame.ip = target;
                }
            }
            Opcode::PopJumpIfFalse => {
                let v = self.vm_pop();
                if !self.vm_is_truthy(&v)? {
                    self.vm_frame().ip = instr.arg as usize;
                }
            }
            Opcode::PopJumpIfTrue => {
                let v = self.vm_pop();
                if self.vm_is_truthy(&v)? {
                    self.vm_frame().ip = instr.arg as usize;
                }
            }
            Opcode::JumpIfTrueOrPop => {
                let v = self.vm_frame().peek().clone();
                if self.vm_is_truthy(&v)? {
                    self.vm_frame().ip = instr.arg as usize;
                } else {
                    self.vm_pop();
                }
            }
            Opcode::JumpIfFalseOrPop => {
                let v = self.vm_frame().peek().clone();
                if !self.vm_is_truthy(&v)? {
                    self.vm_frame().ip = instr.arg as usize;
                } else {
                    self.vm_pop();
                }
            }
            Opcode::GetIter => {
                let obj = self.vm_pop();
                // Dict subclass: use get_iter directly (dict_storage handles it)
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if inst.dict_storage.is_some() {
                        self.vm_push(obj.get_iter()?);
                        return Ok(None);
                    }
                    if inst.attrs.read().contains_key("__deque__") {
                        self.vm_push(obj.get_iter()?);
                        return Ok(None);
                    }
                }
                // Class with __iter__ (e.g. Enum classes): call __iter__()
                if let PyObjectPayload::Class(_) = &obj.payload {
                    // Use get_attr which handles MRO/base class lookup
                    if let Some(iter_method) = obj.get_attr("__iter__") {
                        // Try no-arg call first (staticmethod / stored closure), fall back to cls arg
                        let result = match self.call_object(iter_method.clone(), vec![]) {
                            Ok(r) => r,
                            Err(_) => self.call_object(iter_method, vec![obj.clone()])?,
                        };
                        // If the result is a list, convert it to an iterator
                        if let PyObjectPayload::List(_) = &result.payload {
                            self.vm_push(result.get_iter()?);
                        } else {
                            self.vm_push(result);
                        }
                        return Ok(None);
                    }
                }
                if let Some(r) = self.try_call_dunder(&obj, "__iter__", vec![])? {
                    // If __iter__ returned a list/tuple, convert to proper iterator
                    if matches!(
                        &r.payload,
                        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)
                    ) {
                        self.vm_push(r.get_iter()?);
                    } else {
                        self.vm_push(r);
                    }
                } else {
                    // Builtin base type subclass: delegate to __builtin_value__
                    if let PyObjectPayload::Instance(inst) = &obj.payload {
                        if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                            self.vm_push(bv.get_iter()?);
                            return Ok(None);
                        }
                    }
                    match obj.get_iter() {
                        Ok(iter) => self.vm_push(iter),
                        Err(_) => {
                            // Fall back to __getitem__-based iteration (old-style sequence protocol)
                            if let Some(getitem) = obj.get_attr("__getitem__") {
                                let mut items = Vec::new();
                                let mut idx: i64 = 0;
                                loop {
                                    match self
                                        .call_object(getitem.clone(), vec![PyObject::int(idx)])
                                    {
                                        Ok(val) => {
                                            items.push(val);
                                            idx += 1;
                                        }
                                        Err(e) if e.kind == ExceptionKind::IndexError => break,
                                        Err(e) => return Err(e),
                                    }
                                }
                                self.vm_push(PyObject::list(items).get_iter()?);
                            } else {
                                return Err(PyException::type_error(format!(
                                    "'{}' object is not iterable",
                                    obj.type_name()
                                )));
                            }
                        }
                    }
                }
            }
            Opcode::GetYieldFromIter => {
                // Like GetIter but for yield from — if it's already a generator/coroutine, leave it.
                let obj = self.vm_frame().peek().clone();
                if matches!(
                    &obj.payload,
                    PyObjectPayload::Generator(_)
                        | PyObjectPayload::Coroutine(_)
                        | PyObjectPayload::AsyncGenerator(_)
                        | PyObjectPayload::AsyncGenAwaitable { .. }
                ) {
                    // Already a generator/coroutine, leave on stack
                } else {
                    self.vm_pop();
                    if let Some(r) = self.try_call_dunder(&obj, "__iter__", vec![])? {
                        self.vm_push(r);
                    } else {
                        self.vm_push(obj.get_iter()?);
                    }
                }
            }
            Opcode::ForIter => {
                let iter = self.vm_frame().peek().clone();
                if let PyObjectPayload::Generator(ref gen_arc) = iter.payload {
                    let gen_arc = gen_arc.clone();
                    match self.resume_generator(&gen_arc, PyObject::none()) {
                        Ok(value) => {
                            self.vm_push(value);
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            self.vm_pop(); // remove exhausted generator
                            self.vm_frame().ip = instr.arg as usize;
                        }
                        Err(e) => return Err(e),
                    }
                } else if matches!(
                    &iter.payload,
                    PyObjectPayload::Instance(_) | PyObjectPayload::Module { .. }
                ) {
                    if let Some(next_method) = iter.get_attr("__next__") {
                        let call_args = if matches!(&iter.payload, PyObjectPayload::Module { .. }) {
                            vec![iter.clone()]
                        } else {
                            vec![]
                        };
                        match self.call_object(next_method, call_args) {
                            Ok(value) => {
                                self.vm_push(value);
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                let f = self.vm_frame();
                                f.pop();
                                f.ip = instr.arg as usize;
                            }
                            Err(e) => return Err(e),
                        }
                        return Ok(None);
                    } else {
                        return Err(PyException::type_error("iterator has no __next__ method"));
                    }
                } else if let PyObjectPayload::Iterator(ref iter_data_arc) = iter.payload {
                    // Check for VM-level lazy iterators
                    let needs_vm = {
                        let data = iter_data_arc.read();
                        matches!(
                            &*data,
                            IteratorData::Enumerate { .. }
                                | IteratorData::Zip { .. }
                                | IteratorData::ZipLongest { .. }
                                | IteratorData::Islice { .. }
                                | IteratorData::MapOne { .. }
                                | IteratorData::Map { .. }
                                | IteratorData::Filter { .. }
                                | IteratorData::FilterFalse { .. }
                                | IteratorData::Sentinel { .. }
                                | IteratorData::TakeWhile { .. }
                                | IteratorData::DropWhile { .. }
                                | IteratorData::Count { .. }
                                | IteratorData::Cycle { .. }
                                | IteratorData::Repeat { .. }
                                | IteratorData::Chain { .. }
                                | IteratorData::SeqIter { .. }
                                | IteratorData::Starmap { .. }
                                | IteratorData::Tee { .. }
                                | IteratorData::HeldIter { .. }
                        )
                    };
                    if needs_vm {
                        match self.advance_lazy_iterator(&iter) {
                            Ok(Some(value)) => {
                                self.vm_push(value);
                            }
                            Ok(None) => {
                                let f = self.vm_frame();
                                f.pop();
                                f.ip = instr.arg as usize;
                            }
                            Err(e) => return Err(e),
                        }
                        return Ok(None);
                    }
                    let frame = self.vm_frame();
                    // Iterator mutates in place; skip pop/push of iterator for perf
                    match builtins::iter_next_value(&iter)? {
                        Some(value) => {
                            frame.push(value);
                        }
                        None => {
                            frame.pop(); // remove exhausted iterator
                            frame.ip = instr.arg as usize;
                        }
                    }
                } else {
                    let frame = self.vm_frame();
                    match builtins::iter_next_value(&iter)? {
                        Some(value) => {
                            frame.push(value);
                        }
                        None => {
                            frame.pop(); // remove exhausted iterator
                            frame.ip = instr.arg as usize;
                        }
                    }
                }
            }
            Opcode::EndForLoop => {
                // Pop iterator and close it if it's a generator.
                // Ensures generator finally blocks run on loop break.
                let iter = self.vm_pop();
                if let PyObjectPayload::Generator(ref gen_arc) = iter.payload {
                    let gen = gen_arc.read();
                    if !gen.finished && gen.has_frame() {
                        drop(gen);
                        let gen_arc = gen_arc.clone();
                        match self.gen_throw(
                            &gen_arc,
                            ExceptionKind::GeneratorExit,
                            CompactString::new(""),
                        ) {
                            Ok(_) | Err(_) => {}
                        }
                        let mut gen = gen_arc.write();
                        gen.finished = true;
                        gen.clear_frame();
                    }
                }
            }
            // ForIterStoreFast fallback: do ForIter then StoreFast
            Opcode::ForIterStoreFast => {
                let jump_target = (instr.arg >> 16) as u32;
                let store_idx = (instr.arg & 0xFFFF) as usize;
                let for_instr = Instruction::new(Opcode::ForIter, jump_target);
                self.exec_jump_ops(for_instr)?;
                let needs_drain = {
                    let frame = self.vm_frame();
                    if frame.ip != jump_target as usize {
                        let v = frame.pop();
                        frame.set_local(store_idx, v);
                        ferrython_core::error::has_pending_finalizers()
                    } else {
                        false
                    }
                };
                if needs_drain {
                    self.drain_pending_finalizers();
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
