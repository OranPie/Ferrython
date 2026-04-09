//! Control flow: jumps, iterators, container building, function calls, return/import

use crate::builtins;
use crate::frame::BlockKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::{CodeObject, Instruction};
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    has_descriptor_get, lookup_in_class_mro,
};
use ferrython_core::types::{HashableKey, PyFunction};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

impl VirtualMachine {
    pub(crate) fn exec_jump_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::JumpForward | Opcode::JumpAbsolute => {
                self.vm_frame().ip = instr.arg as usize;
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
                    if matches!(&r.payload, PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)) {
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
                                    match self.call_object(getitem.clone(), vec![PyObject::int(idx)]) {
                                        Ok(val) => { items.push(val); idx += 1; }
                                        Err(e) if e.kind == ExceptionKind::IndexError => break,
                                        Err(e) => return Err(e),
                                    }
                                }
                                self.vm_push(PyObject::list(items).get_iter()?);
                            } else {
                                return Err(PyException::type_error(
                                    format!("'{}' object is not iterable", obj.type_name())
                                ));
                            }
                        }
                    }
                }
            }
            Opcode::GetYieldFromIter => {
                // Like GetIter but for yield from — if it's already a generator/coroutine, leave it.
                let obj = self.vm_frame().peek().clone();
                if matches!(&obj.payload,
                    PyObjectPayload::Generator(_) | PyObjectPayload::Coroutine(_)
                    | PyObjectPayload::AsyncGenerator(_) | PyObjectPayload::AsyncGenAwaitable { .. }
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
                } else if matches!(&iter.payload, PyObjectPayload::Instance(_) | PyObjectPayload::Module { .. }) {
                    if let Some(next_method) = iter.get_attr("__next__") {
                        let call_args = if matches!(&iter.payload, PyObjectPayload::Module { .. }) {
                            vec![iter.clone()]
                        } else {
                            vec![]
                        };
                        match self.call_object(next_method, call_args) {
                            Ok(value) => { self.vm_push(value); }
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
                        let data = iter_data_arc.lock().unwrap();
                        matches!(&*data, IteratorData::Enumerate { .. }
                            | IteratorData::Zip { .. }
                            | IteratorData::Map { .. }
                            | IteratorData::Filter { .. }
                            | IteratorData::Sentinel { .. }
                            | IteratorData::TakeWhile { .. }
                            | IteratorData::DropWhile { .. }
                            | IteratorData::Count { .. }
                            | IteratorData::Cycle { .. }
                            | IteratorData::Repeat { .. }
                            | IteratorData::Chain { .. }
                            | IteratorData::Starmap { .. })
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
                    if !gen.finished && gen.frame.is_some() {
                        drop(gen);
                        let gen_arc = gen_arc.clone();
                        match self.gen_throw(&gen_arc, ExceptionKind::GeneratorExit, String::new()) {
                            Ok(_) | Err(_) => {}
                        }
                        let mut gen = gen_arc.write();
                        gen.finished = true;
                        gen.frame = None;
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

// ── Group 9: Container building ──────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_build_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        match instr.op {
            Opcode::BuildTuple => {
                let count = instr.arg as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count { items.push(frame.pop()); }
                items.reverse();
                frame.push(PyObject::tuple(items));
            }
            Opcode::BuildList => {
                let count = instr.arg as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count { items.push(frame.pop()); }
                items.reverse();
                frame.push(PyObject::list(items));
            }
            Opcode::BuildSet => {
                let count = instr.arg as usize;
                let mut stack_items = Vec::new();
                for _ in 0..count { stack_items.push(frame.pop()); }
                stack_items.reverse();
                // Drop frame borrow before calling vm_to_hashable_key
                let _ = frame;
                let mut set = IndexMap::new();
                for item in stack_items {
                    if let Ok(key) = self.vm_to_hashable_key(&item) {
                        set.insert(key, item);
                    }
                }
                self.vm_frame().push(PyObject::set(set));
            }
            Opcode::BuildMap => {
                let count = instr.arg as usize;
                let mut entries = Vec::new();
                for _ in 0..count {
                    let value = frame.pop();
                    let key = frame.pop();
                    entries.push((key, value));
                }
                entries.reverse();
                let _ = frame;
                let mut map = IndexMap::new();
                for (key, value) in entries {
                    let hkey = self.vm_to_hashable_key(&key)?;
                    map.insert(hkey, value);
                }
                self.vm_frame().push(PyObject::dict(map));
            }
            Opcode::BuildConstKeyMap => {
                let keys_tuple = frame.pop();
                let keys = keys_tuple.to_list()?;
                let count = instr.arg as usize;
                let mut values = Vec::new();
                for _ in 0..count { values.push(frame.pop()); }
                values.reverse();
                let _ = frame;
                let mut map = IndexMap::new();
                for (key, value) in keys.into_iter().zip(values) {
                    let hkey = self.vm_to_hashable_key(&key)?;
                    map.insert(hkey, value);
                }
                self.vm_frame().push(PyObject::dict(map));
            }
            Opcode::BuildString => {
                let count = instr.arg as usize;
                let mut parts = Vec::new();
                for _ in 0..count { parts.push(frame.pop()); }
                parts.reverse();
                let s: String = parts.iter().map(|p| p.py_to_string()).collect();
                frame.push(PyObject::str_val(CompactString::from(s)));
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
                    if let Ok(key) = self.vm_to_hashable_key(&item) {
                        s.write().insert(key, item);
                    }
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
                        m.write().insert(hk, value);
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
                                tgt.insert(k.clone(), v.clone());
                            }
                        }
                        PyObjectPayload::InstanceDict(source) => {
                            let src = source.read();
                            let mut tgt = target.write();
                            for (k, v) in src.iter() {
                                tgt.insert(HashableKey::Str(k.clone()), v.clone());
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
                // Drop frame borrow so we can call __index__ via self
                drop(frame);
                // Resolve __index__ for non-int, non-None slice components
                let start = if !matches!(start.payload, PyObjectPayload::None | PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)) {
                    self.try_call_dunder(&start, "__index__", vec![])?.unwrap_or(start)
                } else { start };
                let stop = if !matches!(stop.payload, PyObjectPayload::None | PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)) {
                    self.try_call_dunder(&stop, "__index__", vec![])?.unwrap_or(stop)
                } else { stop };
                let step = match step {
                    Some(s) if !matches!(s.payload, PyObjectPayload::None | PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)) =>
                        Some(self.try_call_dunder(&s, "__index__", vec![])?.unwrap_or(s)),
                    other => other,
                };
                let s_start = if matches!(start.payload, PyObjectPayload::None) { None } else { Some(start) };
                let s_stop = if matches!(stop.payload, PyObjectPayload::None) { None } else { Some(stop) };
                // Re-borrow frame to push result
                let frame = self.vm_frame();
                frame.push(PyObject::slice(s_start, s_stop, step));
            }
            Opcode::UnpackSequence => {
                let seq = self.vm_pop();
                let items = self.vm_collect_iterable(&seq)?;
                let count = instr.arg as usize;
                if items.len() != count {
                    return Err(PyException::value_error(format!(
                        "not enough values to unpack (expected {}, got {})",
                        count, items.len()
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
                        total_fixed, items.len()
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

// ── Group 10: Function calls ─────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_call_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::CallFunction => {
                let frame = self.vm_frame();
                let arg_count = instr.arg as usize;
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count { args.push(frame.pop()); }
                args.reverse();
                let func = frame.pop();
                let mut result = self.call_object(func, args)?;
                // Post-call intercepts for VM-aware builtins
                result = self.post_call_intercept(result)?;
                self.vm_push(result);
            }
            Opcode::CallFunctionKw => {
                let frame = self.vm_frame();
                let kw_names_obj = frame.pop();
                let arg_count = instr.arg as usize;
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count { args.push(frame.pop()); }
                args.reverse();
                let func = frame.pop();
                let kw_names: Vec<CompactString> = match &kw_names_obj.payload {
                    PyObjectPayload::Tuple(items) => {
                        items.iter().map(|item| {
                            match &item.payload {
                                PyObjectPayload::Str(s) => s.clone(),
                                _ => CompactString::from(item.py_to_string()),
                            }
                        }).collect()
                    }
                    _ => Vec::new(),
                };
                let n_kw = kw_names.len();
                let n_pos = arg_count - n_kw;
                let pos_args = args[..n_pos].to_vec();
                let mut kwargs: Vec<(CompactString, PyObjectRef)> = Vec::new();
                for (i, name) in kw_names.iter().enumerate() {
                    kwargs.push((name.clone(), args[n_pos + i].clone()));
                }
                let mut result = self.call_object_kw(func, pos_args, kwargs)?;
                result = self.post_call_intercept(result)?;
                self.vm_push(result);
            }
            Opcode::CallMethod => {
                let frame = self.vm_frame();
                let arg_count = instr.arg as usize;
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count { args.push(frame.pop()); }
                args.reverse();
                let method = frame.pop();
                let mut result = self.call_object(method, args)?;
                result = self.post_call_intercept(result)?;
                self.vm_push(result);
            }
            Opcode::CallFunctionEx => {
                let frame = self.vm_frame();
                let has_kwargs = (instr.arg & 1) != 0;
                let kwargs_obj = if has_kwargs { Some(frame.pop()) } else { None };
                let args_obj = frame.pop();
                let func = frame.pop();
                // Use collect_iterable to handle generators and lazy iterators
                let pos_args = self.collect_iterable(&args_obj)?;
                if let Some(kw_obj) = kwargs_obj {
                    let mut kw_vec: Vec<(CompactString, PyObjectRef)> = Vec::new();
                    match &kw_obj.payload {
                        PyObjectPayload::Dict(map) => {
                            for (k, v) in map.read().iter() {
                                let name = match k {
                                    HashableKey::Str(s) => s.clone(),
                                    _ => CompactString::from(format!("{:?}", k)),
                                };
                                kw_vec.push((name, v.clone()));
                            }
                        }
                        PyObjectPayload::InstanceDict(map) => {
                            for (k, v) in map.read().iter() {
                                kw_vec.push((k.clone(), v.clone()));
                            }
                        }
                        _ => {}
                    }
                    let result = self.call_object_kw(func, pos_args, kw_vec)?;
                    self.vm_push(result);
                } else {
                    let result = self.call_object(func, pos_args)?;
                    self.vm_push(result);
                }
            }
            Opcode::LoadMethod => {
                let frame = self.vm_frame();
                let name = frame.code.names[instr.arg as usize].clone();
                let obj = frame.pop();

                // Fast path for simple instance method calls:
                // Skip __getattribute__ check and descriptor protocol for plain instances
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    let skip_ga = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        !cd.has_getattribute
                    } else {
                        false
                    };
                    // Only use fast path for plain instances (no dict_storage, no special markers)
                    let is_plain = skip_ga
                        && inst.dict_storage.is_none()
                        && !inst.is_special
                        && name.as_str() != "__class__"
                        && name.as_str() != "__dict__";

                    if is_plain {
                        // Instance dict lookup
                        if let Some(v) = inst.attrs.read().get(name.as_str()) {
                            frame.push(v.clone());
                            return Ok(None);
                        }
                        // Class MRO lookup (uses method cache)
                        if let Some(method) = lookup_in_class_mro(&inst.class, &name) {
                            match &method.payload {
                                PyObjectPayload::Function(_)
                                | PyObjectPayload::NativeFunction { .. } => {
                                    frame.push(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: obj,
                                            method,
                                        }
                                    }));
                                    return Ok(None);
                                }
                                // NativeClosure in class namespace: bind only if marked as method
                                // (name contains '.' like "ClassName.method")
                                PyObjectPayload::NativeClosure { ref name, .. } if name.contains('.') => {
                                    frame.push(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: obj,
                                            method,
                                        }
                                    }));
                                    return Ok(None);
                                }
                                PyObjectPayload::StaticMethod(func) => {
                                    frame.push(func.clone());
                                    return Ok(None);
                                }
                                PyObjectPayload::ClassMethod(func) => {
                                    frame.push(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: inst.class.clone(),
                                            method: func.clone(),
                                        }
                                    }));
                                    return Ok(None);
                                }
                                PyObjectPayload::Property { fget, .. } => {
                                    if let Some(getter) = fget {
                                        let result = self.call_object(getter.clone(), vec![obj])?;
                                        self.vm_push(result);
                                        return Ok(None);
                                    }
                                    return Err(PyException::attribute_error(format!(
                                        "unreadable attribute '{}'", name
                                    )));
                                }
                                // lru_cache wrapper (Instance with __wrapped__) → bind self
                                PyObjectPayload::Instance(ref ci) if ci.attrs.read().contains_key("__wrapped__") => {
                                    frame.push(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: obj,
                                            method,
                                        }
                                    }));
                                    return Ok(None);
                                }
                                // For descriptors or other types, fall through to full path
                                _ => {
                                    frame.push(method);
                                    return Ok(None);
                                }
                            }
                        }
                        // Fall through to full get_attr for __getattr__ and other cases
                    }
                }

                // Full path: handles all cases including __getattribute__, special instances, etc.
                match obj.get_attr(&name) {
                    Some(method) => {
                        if matches!(&obj.payload, PyObjectPayload::Module(_))
                            && matches!(&method.payload, PyObjectPayload::NativeFunction { .. })
                            && obj.get_attr("_bind_methods").is_some()
                        {
                            frame.push(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj,
                                    method,
                                }
                            }));
                        } else {
                            frame.push(method);
                        }
                    }
                    None => {
                        // Fallback: resolve_type_class_method for BuiltinType
                        let type_name = match &obj.payload {
                            PyObjectPayload::BuiltinType(tn) => Some(tn.as_str()),
                            PyObjectPayload::NativeFunction { name: fn_name, .. } => Some(fn_name.as_str()),
                            _ => None,
                        };
                        if let Some(tn) = type_name {
                            if let Some(type_method) = crate::builtins::resolve_type_class_method(tn, &name) {
                                self.vm_push(type_method);
                                return Ok(None);
                            }
                        }
                        // Fallback: check __getattr__ on instances
                        if let PyObjectPayload::Instance(_) = &obj.payload {
                            if let Some(ga) = obj.get_attr("__getattr__") {
                                let name_arg = PyObject::str_val(CompactString::from(name.as_str()));
                                let result = self.call_object(ga, vec![name_arg])?;
                                self.vm_push(result);
                                return Ok(None);
                            }
                        }
                        return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), name
                        )));
                    }
                }
            }
            Opcode::MakeFunction => {
                let frame = self.vm_frame();
                let qualname = frame.pop();
                let code_obj = frame.pop();
                let flags = instr.arg;
                let closure_cells = if flags & 0x08 != 0 {
                    let closure_tuple = frame.pop();
                    match &closure_tuple.payload {
                        PyObjectPayload::Tuple(items) => {
                            items.iter().map(|item| {
                                match &item.payload {
                                    PyObjectPayload::Cell(cell) => cell.clone(),
                                    _ => Arc::new(RwLock::new(Some(item.clone()))),
                                }
                            }).collect()
                        }
                        _ => Vec::new(),
                    }
                } else { Vec::new() };
                let mut annotations = IndexMap::new();
                if flags & 0x04 != 0 {
                    let ann_obj = frame.pop();
                    if let PyObjectPayload::Dict(m) = &ann_obj.payload {
                        for (k, v) in m.read().iter() {
                            if let HashableKey::Str(name) = k {
                                annotations.insert(name.clone(), v.clone());
                            }
                        }
                    }
                }
                let kw_defaults = if flags & 0x02 != 0 {
                    let kwd_obj = frame.pop();
                    if let PyObjectPayload::Dict(m) = &kwd_obj.payload {
                        let mut result = IndexMap::new();
                        for (k, v) in m.read().iter() {
                            if let HashableKey::Str(name) = k {
                                result.insert(name.clone(), v.clone());
                            }
                        }
                        result
                    } else {
                        IndexMap::new()
                    }
                } else { IndexMap::new() };
                let mut defaults = Vec::new();
                if flags & 0x01 != 0 {
                    let default_tuple = frame.pop();
                    defaults = default_tuple.to_list().unwrap_or_default();
                }
                let code: Arc<CodeObject> = match &code_obj.payload {
                    PyObjectPayload::Code(c) => Arc::clone(c),
                    _ => return Err(PyException::type_error(
                        "expected code object for MAKE_FUNCTION",
                    )),
                };
                let qualname_str = qualname.as_str().map(CompactString::from)
                    .unwrap_or_else(|| code.name.clone());
                let constant_cache = Arc::new(PyFunction::build_constant_cache(&code));
                let func = PyFunction {
                    name: code.name.clone(),
                    qualname: qualname_str,
                    code,
                    constant_cache,
                    defaults,
                    kw_defaults,
                    globals: frame.globals.clone(),
                    closure: closure_cells,
                    annotations,
                    attrs: Arc::new(RwLock::new(IndexMap::new())),
                };
                frame.push(PyObject::function(func));
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

// ── Group 11: Return + Import ────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_return_import(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::ReturnValue => {
                let frame = self.vm_frame();
                let value = frame.pop();
                // If inside a finally block, the new return overrides any pending return
                let mut found_finally = false;
                while let Some(block) = frame.block_stack.last() {
                    if block.kind == BlockKind::Finally {
                        let handler = block.handler;
                        frame.block_stack.pop();
                        frame.pending_return = Some(value.clone());
                        frame.push(PyObject::none());
                        frame.ip = handler;
                        found_finally = true;
                        break;
                    } else {
                        frame.block_stack.pop();
                    }
                }
                if !found_finally {
                    // Return immediately — new return value overrides any pending
                    return Ok(Some(value));
                }
            }
            Opcode::ImportName => {
                let frame = self.vm_frame();
                let fromlist = frame.pop();
                let level_obj = frame.pop();
                let level = level_obj.as_int().unwrap_or(0) as usize;
                let name = frame.code.names[instr.arg as usize].clone();
                let filename = frame.code.filename.clone();
                let has_fromlist = !matches!(&fromlist.payload, PyObjectPayload::None);

                let module = self.import_module_dotted(&name, level, has_fromlist, &filename)?;
                self.vm_push(module);
                return Ok(None);
            }
            Opcode::ImportFrom => {
                let (name, module, mod_name, mod_file, filename) = {
                    let frame = self.vm_frame();
                    let name = frame.code.names[instr.arg as usize].clone();
                    let module = frame.peek().clone();
                    // Prefer __name__, but fall back to __package__ for relative imports
                    let raw_name = module.get_attr("__name__")
                        .map(|n| n.py_to_string())
                        .unwrap_or_default();
                    let mod_name = if raw_name == "<package>" || raw_name.is_empty() {
                        // Use __package__ or derive from __file__
                        module.get_attr("__package__")
                            .map(|p| p.py_to_string())
                            .filter(|s| !s.is_empty())
                            .or_else(|| {
                                module.get_attr("__file__").map(|f| {
                                    let fp = f.py_to_string();
                                    let path = std::path::Path::new(&fp);
                                    let is_init = path.file_name().map(|f| f == "__init__.py").unwrap_or(false);
                                    if is_init {
                                        path.parent()
                                            .and_then(|p| p.file_name())
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("")
                                            .to_string()
                                    } else {
                                        path.file_stem()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("")
                                            .to_string()
                                    }
                                })
                            })
                            .unwrap_or(raw_name)
                    } else {
                        raw_name
                    };
                    let mod_file = module.get_attr("__file__")
                        .map(|f| f.py_to_string())
                        .unwrap_or_else(|| "unknown location".to_string());
                    let filename = frame.code.filename.clone();
                    (name, module, mod_name, mod_file, filename)
                };
                match module.get_attr(&name) {
                    Some(v) => {
                        // Descriptor protocol: if the value has __get__ and was found
                        // via class lookup (not instance dict), invoke __get__.
                        // This handles six.moves lazy descriptors.
                        if has_descriptor_get(&v) {
                            if let Some(get_method) = v.get_attr("__get__") {
                                let (instance_arg, owner_arg) = match &module.payload {
                                    PyObjectPayload::Instance(inst) => (module.clone(), inst.class.clone()),
                                    _ => (module.clone(), PyObject::none()),
                                };
                                match self.call_object(get_method, vec![instance_arg, owner_arg]) {
                                    Ok(result) => { self.vm_frame().push(result); }
                                    Err(_) => { self.vm_frame().push(v); }
                                }
                            } else {
                                self.vm_frame().push(v);
                            }
                        } else {
                            self.vm_frame().push(v);
                        }
                    }
                    None => {
                        // PEP 562: module-level __getattr__ for ImportFrom
                        if let PyObjectPayload::Module(_) = &module.payload {
                            if let Some(ga) = module.get_attr("__getattr__") {
                                let name_arg = PyObject::str_val(CompactString::from(name.as_str()));
                                if let Ok(result) = self.call_object(ga, vec![name_arg]) {
                                    self.vm_frame().push(result);
                                    return Ok(None);
                                }
                            }
                        }
                        // CPython fallback: try importing package.submodule
                        if !mod_name.is_empty() {
                            let submod_name = format!("{}.{}", mod_name, name);
                            // Use the correct search root: for packages (__init__.py),
                            // the importer must be the parent of the package directory
                            // so "urllib3/exceptions" resolves relative to site-packages/
                            let search_file = if mod_file.ends_with("__init__.py") {
                                // Go up two levels: __init__.py -> pkg_dir -> parent
                                let p = std::path::Path::new(&mod_file);
                                p.parent()
                                    .and_then(|pkg| pkg.parent())
                                    .map(|root| root.join("__importer__").to_string_lossy().to_string())
                                    .unwrap_or_else(|| filename.to_string())
                            } else {
                                filename.to_string()
                            };
                            match self.import_module_dotted(&submod_name, 0, true, &search_file) {
                                Ok(submod) => {
                                    match &module.payload {
                                        PyObjectPayload::Module(md) => {
                                            md.attrs.write().insert(name.clone(), submod.clone());
                                        }
                                        PyObjectPayload::Instance(inst) => {
                                            inst.attrs.write().insert(name.clone(), submod.clone());
                                        }
                                        _ => {}
                                    }
                                    self.vm_frame().push(submod);
                                }
                                Err(_e) => {
                                    // If the error itself is an ImportError for a name inside the submodule,
                                    // bubble it up rather than wrapping it.
                                    if _e.kind == ferrython_core::error::ExceptionKind::ImportError {
                                        let msg = _e.message.clone();
                                        if msg.starts_with("cannot import name") && !msg.contains(&format!("'{}'", name)) {
                                            return Err(_e);
                                        }
                                    }
                                    return Err(PyException::import_error(format!(
                                        "cannot import name '{}' from '{}' ({})",
                                        name, mod_name, mod_file
                                    )));
                                }
                            }
                        } else {
                            return Err(PyException::import_error(format!(
                                "cannot import name '{}' from module", name
                            )));
                        }
                    }
                }
            }
            Opcode::ImportStar => {
                let frame = self.vm_frame();
                let module = frame.pop();
                if let PyObjectPayload::Module(mod_data) = &module.payload {
                    let attrs = mod_data.attrs.read();
                    let all_names: Option<Vec<String>> = attrs.get("__all__").and_then(|v| {
                        v.to_list().ok().map(|items| items.iter().map(|x: &PyObjectRef| x.py_to_string()).collect::<Vec<String>>())
                    });
                    let mut globals = frame.globals.write();
                    for (k, v) in attrs.iter() {
                        if k.starts_with('_') && all_names.is_none() { continue; }
                        if let Some(ref names) = all_names {
                            if !names.contains(&k.to_string()) { continue; }
                        }
                        globals.insert(k.clone(), v.clone());
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

