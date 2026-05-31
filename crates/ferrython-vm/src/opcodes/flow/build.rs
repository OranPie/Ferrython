//! Container-building opcode handlers.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::helpers::mark_dict_storage_mutated;
use ferrython_core::object::{
    FxHashKeyMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── Group 9: Container building ──────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_build_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        match instr.op {
            Opcode::BuildTuple => {
                let count = instr.arg as usize;
                if count == 0 {
                    frame.push(PyObject::tuple(vec![]));
                } else {
                    let start = frame.stack.len() - count;
                    let items = frame.stack.split_off(start);
                    frame.push(PyObject::tuple(items));
                }
            }
            Opcode::BuildList => {
                let count = instr.arg as usize;
                if count == 0 {
                    frame.push(PyObject::list(vec![]));
                } else {
                    let start = frame.stack.len() - count;
                    let items = frame.stack.split_off(start);
                    frame.push(PyObject::list(items));
                }
            }
            Opcode::BuildSet => {
                let count = instr.arg as usize;
                let mut stack_items = Vec::new();
                for _ in 0..count {
                    stack_items.push(frame.pop());
                }
                stack_items.reverse();
                // Drop frame borrow before calling vm_to_hashable_key
                let _ = frame;
                let mut set = IndexMap::new();
                for item in stack_items {
                    let key = self.vm_to_hashable_key(&item)?;
                    set.entry(key).or_insert(item);
                }
                self.vm_frame().push(PyObject::set(set));
            }
            Opcode::BuildMap => {
                let count = instr.arg as usize;
                let pair_count = count * 2;
                let start = frame.stack.len() - pair_count;
                let _ = frame;
                let mut map = FxHashKeyMap::with_capacity_and_hasher(count, Default::default());
                for i in 0..count {
                    let frame = self.vm_frame();
                    let key = frame.stack[start + i * 2].clone();
                    let value = frame.stack[start + i * 2 + 1].clone();
                    let hkey = self.vm_to_hashable_key(&key)?;
                    map.insert(hkey, value);
                }
                let frame = self.vm_frame();
                frame.stack.truncate(start);
                frame.push(PyObject::dict_fx(map));
            }
            Opcode::BuildConstKeyMap => {
                let keys_tuple = frame.pop();
                let keys = keys_tuple.to_list()?;
                let count = instr.arg as usize;
                let start = frame.stack.len() - count;
                let _ = frame;
                let mut map = FxHashKeyMap::with_capacity_and_hasher(count, Default::default());
                for (i, key) in keys.into_iter().enumerate() {
                    let value = self.vm_frame().stack[start + i].clone();
                    let hkey = self.vm_to_hashable_key(&key)?;
                    map.insert(hkey, value);
                }
                let frame = self.vm_frame();
                frame.stack.truncate(start);
                frame.push(PyObject::dict_fx(map));
            }
            Opcode::BuildString => {
                let count = instr.arg as usize;
                if count == 0 {
                    frame.push(PyObject::str_val(CompactString::from("")));
                } else if count == 1 {
                    // Single item — already a string from FormatValue
                } else {
                    let start = frame.stack.len() - count;
                    // Fast path: all items are already Str
                    let mut total_len = 0usize;
                    let mut all_str = true;
                    for i in start..frame.stack.len() {
                        if let PyObjectPayload::Str(s) = &frame.stack[i].payload {
                            total_len += s.len();
                        } else {
                            all_str = false;
                            break;
                        }
                    }
                    if all_str {
                        let mut result = String::with_capacity(total_len);
                        for i in start..frame.stack.len() {
                            if let PyObjectPayload::Str(s) = &frame.stack[i].payload {
                                result.push_str(s.as_str());
                            }
                        }
                        frame.stack.truncate(start);
                        frame.push(PyObject::str_val(CompactString::from(result)));
                    } else {
                        let mut parts = Vec::new();
                        for _ in 0..count {
                            parts.push(frame.pop());
                        }
                        parts.reverse();
                        let s: String = parts.iter().map(|p| p.py_to_string()).collect();
                        frame.push(PyObject::str_val(CompactString::from(s)));
                    }
                }
            }
            Opcode::ListAppend => {
                let item = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let list_obj = frame.stack[stack_pos].clone();
                if let PyObjectPayload::List(items) = &list_obj.payload {
                    items.write().push(item);
                }
            }
            Opcode::SetAdd => {
                let item = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let set_obj = frame.stack[stack_pos].clone();
                let _ = frame;
                if let PyObjectPayload::Set(s) = &set_obj.payload {
                    let key = self.vm_to_hashable_key(&item)?;
                    s.write().entry(key).or_insert(item);
                }
                // frame not needed after this
            }
            Opcode::MapAdd => {
                let value = frame.pop();
                let key = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let dict_obj = frame.stack[stack_pos].clone();
                let _ = frame;
                if let PyObjectPayload::Dict(m) = &dict_obj.payload {
                    if let Ok(hk) = self.vm_to_hashable_key(&key) {
                        if m.write().insert(hk, value).is_none() {
                            mark_dict_storage_mutated(m);
                        }
                    }
                }
            }
            Opcode::DictUpdate | Opcode::DictMerge => {
                let update_obj = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let dict_obj = &frame.stack[stack_pos];
                if let PyObjectPayload::Dict(target) = &dict_obj.payload {
                    match &update_obj.payload {
                        PyObjectPayload::Dict(source) => {
                            let src = source.read();
                            let mut tgt = target.write();
                            for (k, v) in src.iter() {
                                if tgt.insert(k.clone(), v.clone()).is_none() {
                                    mark_dict_storage_mutated(target);
                                }
                            }
                        }
                        PyObjectPayload::InstanceDict(source) => {
                            let src = ferrython_core::object::helpers::instance_dict_as_hashkey_map(
                                source,
                            );
                            let mut tgt = target.write();
                            for (k, v) in src {
                                if tgt.insert(k, v).is_none() {
                                    mark_dict_storage_mutated(target);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Opcode::ListExtend => {
                let iterable = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let list_obj = frame.stack[stack_pos].clone();
                if let PyObjectPayload::List(items) = &list_obj.payload {
                    if let PyObjectPayload::Generator(gen_arc) = &iterable.payload {
                        // Consume generator by driving it through the VM
                        loop {
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(val) => items.write().push(val),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        }
                    } else {
                        // Try to_list first, fall back to collect_iterable for custom __iter__
                        let new_items = match iterable.to_list() {
                            Ok(v) => v,
                            Err(_) => self.collect_iterable(&iterable)?,
                        };
                        items.write().extend(new_items);
                    }
                }
            }
            Opcode::SetUpdate => {
                let iterable = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let set_obj = frame.stack[stack_pos].clone();
                if let PyObjectPayload::Set(s) = &set_obj.payload {
                    let items = if let PyObjectPayload::Generator(gen_arc) = &iterable.payload {
                        let mut result = Vec::new();
                        loop {
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(val) => result.push(val),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        }
                        result
                    } else {
                        match iterable.to_list() {
                            Ok(v) => v,
                            Err(_) => self.collect_iterable(&iterable)?,
                        }
                    };
                    let mut set = s.write();
                    for item in items {
                        if let Ok(key) = self.vm_to_hashable_key(&item) {
                            set.insert(key, item);
                        }
                    }
                }
            }
            Opcode::ListToTuple => {
                let list = frame.pop();
                let items = list.to_list()?;
                frame.push(PyObject::tuple(items));
            }
            Opcode::BuildSlice => {
                let argc = instr.arg as usize;
                let step = if argc == 3 { Some(frame.pop()) } else { None };
                let stop = frame.pop();
                let start = frame.pop();
                let s_start = if matches!(start.payload, PyObjectPayload::None) {
                    None
                } else {
                    Some(start)
                };
                let s_stop = if matches!(stop.payload, PyObjectPayload::None) {
                    None
                } else {
                    Some(stop)
                };
                frame.push(PyObject::slice(s_start, s_stop, step));
            }
            Opcode::UnpackSequence => {
                let seq = self.vm_pop();
                let items = self.vm_collect_iterable(&seq)?;
                let count = instr.arg as usize;
                if items.len() != count {
                    return Err(PyException::value_error(format!(
                        "not enough values to unpack (expected {}, got {})",
                        count,
                        items.len()
                    )));
                }
                let frame = self.vm_frame();
                for item in items.into_iter().rev() {
                    frame.push(item);
                }
            }
            Opcode::UnpackEx => {
                let seq = self.vm_pop();
                let items = self.vm_collect_iterable(&seq)?;
                let before = (instr.arg & 0xFF) as usize;
                let after = ((instr.arg >> 8) & 0xFF) as usize;
                let total_fixed = before + after;
                if items.len() < total_fixed {
                    return Err(PyException::value_error(format!(
                        "not enough values to unpack (expected at least {}, got {})",
                        total_fixed,
                        items.len()
                    )));
                }
                let star_count = items.len() - total_fixed;
                let frame = self.vm_frame();
                for i in (0..after).rev() {
                    let idx = before + star_count + i;
                    frame.push(items[idx].clone());
                }
                let starred: Vec<PyObjectRef> = items[before..before + star_count].to_vec();
                frame.push(PyObject::list(starred));
                for i in (0..before).rev() {
                    frame.push(items[i].clone());
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
