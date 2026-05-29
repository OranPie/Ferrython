//! Function and method call opcode handlers.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::{CodeObject, Instruction};
use ferrython_core::error::PyException;
use ferrython_core::object::{
    has_descriptor_get, lookup_in_class_mro, FxAttrMap, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyFunction};
use indexmap::IndexMap;
use std::rc::Rc;

// ── Group 10: Function calls ─────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_call_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::CallFunction => {
                let frame = self.vm_frame();
                let arg_count = instr.arg as usize;
                // Drain args directly in forward order (avoids pop+reverse)
                let stack_len = frame.stack.len();
                if stack_len < arg_count + 1 {
                    return Err(PyException::runtime_error(
                        format!("internal error: not enough values on the stack for call (need {}, have {})",
                                arg_count + 1, stack_len)
                    ));
                }
                let args_start = stack_len - arg_count;
                let args: Vec<PyObjectRef> = frame.stack.drain(args_start..).collect();
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
                // Drain args directly in forward order (avoids pop+reverse)
                let stack_len = frame.stack.len();
                if stack_len < arg_count + 1 {
                    return Err(PyException::runtime_error(
                        format!("internal error: not enough values on the stack for call (need {}, have {})",
                                arg_count + 1, stack_len)
                    ));
                }
                let args_start = stack_len - arg_count;
                let args: Vec<PyObjectRef> = frame.stack.drain(args_start..).collect();
                let func = frame.pop();
                let kw_names: Vec<CompactString> = match &kw_names_obj.payload {
                    PyObjectPayload::Tuple(items) => items
                        .iter()
                        .map(|item| match &item.payload {
                            PyObjectPayload::Str(s) => s.to_compact_string(),
                            _ => CompactString::from(item.py_to_string()),
                        })
                        .collect(),
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
            // Two-item stack protocol: stack = [..., slot_0, slot_1, arg0, ..., argN-1]
            // slot_0 = method (non-None) or None sentinel
            // slot_1 = receiver (fast) or callable (slow)
            Opcode::CallMethod => {
                let arg_count = instr.arg as usize;
                // Phase 1: peek at slot_0 to determine if unbound method + check for direct frame creation
                let fast_data = {
                    let frame = self.call_stack.last().unwrap();
                    let stack_len = frame.stack.len();
                    let base_idx = stack_len - arg_count - 2;
                    let slot_0 = &frame.stack[base_idx];
                    if !matches!(&slot_0.payload, PyObjectPayload::None) {
                        // Unbound method path: slot_0 = method
                        if let PyObjectPayload::Function(pf) = &slot_0.payload {
                            if pf.is_simple && pf.code.arg_count as usize == arg_count + 1 {
                                Some((
                                    Rc::clone(&pf.code),
                                    pf.globals.clone(),
                                    Rc::clone(&pf.constant_cache),
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                let result = if let Some((code, globals, cc)) = fast_data {
                    // Super-fast: direct frame creation, move args from stack
                    let mut new_frame = crate::frame::Frame::new_from_pool(
                        code,
                        globals,
                        self.builtins.clone(),
                        cc,
                        &mut self.frame_pool,
                    );
                    {
                        let frame = self.call_stack.last_mut().unwrap();
                        let arg_start = frame.stack.len() - arg_count;
                        for (i, arg) in frame.stack.drain(arg_start..).enumerate() {
                            new_frame.locals[i + 1] = Some(arg);
                        }
                        new_frame.locals[0] = frame.stack.pop(); // receiver (slot_1) — moved, not cloned
                        frame.stack.pop(); // drop method (slot_0)
                    }
                    new_frame.scope_kind = crate::frame::ScopeKind::Function;
                    self.call_stack.push(new_frame);
                    if self.call_stack.len() > self.recursion_limit {
                        if let Some(f) = self.call_stack.pop() {
                            f.recycle(&mut self.frame_pool);
                        }
                        return Err(PyException::recursion_error(
                            "maximum recursion depth exceeded",
                        ));
                    }
                    let r = self.run_frame();
                    if let Some(f) = self.call_stack.pop() {
                        f.recycle(&mut self.frame_pool);
                    }
                    r?
                } else {
                    // General path: pop all items from two-item protocol
                    let frame = self.vm_frame();
                    // Drain args directly in forward order (avoids pop+reverse)
                    let stack_len = frame.stack.len();
                    let args_start = stack_len - arg_count;
                    let args: Vec<PyObjectRef> = frame.stack.drain(args_start..).collect();
                    let slot_1 = frame.pop(); // receiver or callable
                    let slot_0 = frame.pop(); // method or None sentinel

                    if matches!(&slot_0.payload, PyObjectPayload::None) {
                        // Slow path: slot_1 is the callable
                        if let PyObjectPayload::BoundMethod {
                            ref receiver,
                            ref method,
                        } = slot_1.payload
                        {
                            let mut bound_args = vec![receiver.clone()];
                            bound_args.extend(args);
                            self.call_object(method.clone(), bound_args)?
                        } else {
                            self.call_object(slot_1, args)?
                        }
                    } else {
                        // Unbound method: slot_0 = method, slot_1 = receiver
                        let mut bound_args = vec![slot_1];
                        bound_args.extend(args);
                        self.call_object(slot_0, bound_args)?
                    }
                };
                let result = self.post_call_intercept(result)?;
                self.vm_push(result);
            }
            Opcode::CallMethodPopTop => {
                // Same as CallMethod but discard result (fused PopTop)
                let cm_instr = Instruction::new(Opcode::CallMethod, instr.arg);
                // Reuse CallMethod logic via exec_call_ops recursion
                self.exec_call_ops(cm_instr)?;
                // Pop the result that CallMethod pushed
                let frame = self.call_stack.last_mut().unwrap();
                if !frame.stack.is_empty() {
                    frame.pop();
                }
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
                                    HashableKey::Str(s) => s.to_compact_string(),
                                    _ => {
                                        return Err(PyException::type_error(
                                            "keywords must be strings",
                                        ));
                                    }
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
            // Two-item stack protocol: LoadMethod pushes exactly 2 items.
            // Fast path (unbound method found): push method, then receiver
            // Slow path (callable/attr): push None sentinel, then callable
            // CallMethod checks slot_0 to distinguish.
            Opcode::LoadMethod => {
                let frame = self.vm_frame();
                let name = frame.code.names[instr.arg as usize].clone();
                let obj = frame.pop();

                // Fast path for simple instance method calls
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    let skip_ga = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        !cd.has_getattribute
                    } else {
                        false
                    };
                    let is_plain = skip_ga
                        && inst.dict_storage.is_none()
                        && !inst.is_special
                        && !inst.attrs.read().contains_key("__deque__")
                        && name.as_str() != "__class__"
                        && name.as_str() != "__dict__";

                    if is_plain {
                        let effective_class = inst.class.clone();

                        // Class namespace function → two-item: method + receiver
                        if let PyObjectPayload::Class(cd) = &effective_class.payload {
                            if let Some(class_val) = cd.namespace.read().get(name.as_str()).cloned()
                            {
                                if matches!(&class_val.payload, PyObjectPayload::Function(_)) {
                                    frame.push(class_val);
                                    frame.push(obj);
                                    return Ok(None);
                                }
                            }
                        }

                        // Instance dict → two-item slow: None + value
                        if let Some(v) = inst.attrs.read().get(name.as_str()) {
                            let v = v.clone();
                            frame.push(PyObject::none());
                            frame.push(v);
                            return Ok(None);
                        }
                        // MRO lookup
                        if let Some(method) = lookup_in_class_mro(&effective_class, &name) {
                            match &method.payload {
                                PyObjectPayload::Function(_) => {
                                    // Two-item: method + receiver
                                    frame.push(method);
                                    frame.push(obj);
                                    return Ok(None);
                                }
                                PyObjectPayload::NativeFunction(nf) => {
                                    let binds_to_class = if let PyObjectPayload::Class(cd) =
                                        &effective_class.payload
                                    {
                                        let native_name = nf.name.as_str();
                                        let expected_len = cd.name.len() + name.len() + 1;
                                        native_name.len() == expected_len
                                            && native_name.starts_with(cd.name.as_str())
                                            && native_name.as_bytes().get(cd.name.len())
                                                == Some(&b'.')
                                            && &native_name[cd.name.len() + 1..] == name.as_str()
                                    } else {
                                        false
                                    };
                                    if binds_to_class {
                                        frame.push(method);
                                        frame.push(obj);
                                    } else {
                                        frame.push(PyObject::none());
                                        frame.push(method);
                                    }
                                    return Ok(None);
                                }
                                PyObjectPayload::NativeClosure(ref nc) if nc.name.contains('.') => {
                                    frame.push(method);
                                    frame.push(obj);
                                    return Ok(None);
                                }
                                PyObjectPayload::StaticMethod(func) => {
                                    frame.push(PyObject::none());
                                    frame.push(func.clone());
                                    return Ok(None);
                                }
                                PyObjectPayload::ClassMethod(func) => {
                                    // Two-item: func + class-as-receiver
                                    frame.push(func.clone());
                                    frame.push(effective_class.clone());
                                    return Ok(None);
                                }
                                _ if ferrython_core::object::is_property_like(&method) => {
                                    if let Some(getter) =
                                        ferrython_core::object::property_field(&method, "fget")
                                    {
                                        if matches!(&getter.payload, PyObjectPayload::None) {
                                            return Err(PyException::attribute_error(format!(
                                                "unreadable attribute '{}'",
                                                name
                                            )));
                                        }
                                        let getter = crate::builtins::unwrap_abstract_fget(&getter);
                                        let result = self.call_object(getter, vec![obj])?;
                                        let frame = self.vm_frame();
                                        frame.push(PyObject::none());
                                        frame.push(result);
                                        return Ok(None);
                                    }
                                    return Err(PyException::attribute_error(format!(
                                        "unreadable attribute '{}'",
                                        name
                                    )));
                                }
                                PyObjectPayload::Instance(ref ci)
                                    if ci.attrs.read().contains_key("__wrapped__") =>
                                {
                                    frame.push(method);
                                    frame.push(obj);
                                    return Ok(None);
                                }
                                _ if has_descriptor_get(&method) => {
                                    let get_fn = method.get_attr("__get__").unwrap();
                                    let owner = effective_class.clone();
                                    let get_bound = if matches!(
                                        &get_fn.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        get_fn
                                    } else {
                                        PyObjectRef::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: method.clone(),
                                                method: get_fn,
                                            },
                                        })
                                    };
                                    let result =
                                        self.call_object(get_bound, vec![obj.clone(), owner])?;
                                    let frame = self.vm_frame();
                                    frame.push(PyObject::none());
                                    frame.push(result);
                                    return Ok(None);
                                }
                                _ => {
                                    frame.push(PyObject::none());
                                    frame.push(method);
                                    return Ok(None);
                                }
                            }
                        }
                        // Fall through to full get_attr
                    }
                }

                // Full path: handles __getattribute__, special instances, etc.
                match obj.get_attr(&name) {
                    Some(method) => {
                        if matches!(&obj.payload, PyObjectPayload::Module(_))
                            && matches!(&method.payload, PyObjectPayload::NativeFunction(_))
                            && obj.get_attr("_bind_methods").is_some()
                        {
                            // Module method binding → two-item: method + module
                            frame.push(method);
                            frame.push(obj);
                        } else if matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_))
                        {
                            frame.push(method);
                            frame.push(obj);
                        } else {
                            // Already-resolved callable → slow path
                            frame.push(PyObject::none());
                            frame.push(method);
                        }
                    }
                    None => {
                        let type_name = match &obj.payload {
                            PyObjectPayload::BuiltinType(tn) => Some(tn.as_str()),
                            PyObjectPayload::NativeFunction(nf) => Some(nf.name.as_str()),
                            _ => None,
                        };
                        if let Some(tn) = type_name {
                            if let Some(type_method) =
                                crate::builtins::resolve_type_class_method(tn, &name)
                            {
                                self.vm_frame().push(PyObject::none());
                                self.vm_push(type_method);
                                return Ok(None);
                            }
                        }
                        if let PyObjectPayload::Instance(_) = &obj.payload {
                            if let Some(ga) = obj.get_attr("__getattr__") {
                                let name_arg =
                                    PyObject::str_val(CompactString::from(name.as_str()));
                                let result = self.call_object(ga, vec![name_arg])?;
                                let frame = self.vm_frame();
                                frame.push(PyObject::none());
                                frame.push(result);
                                return Ok(None);
                            }
                        }
                        return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'",
                            obj.type_name(),
                            name
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
                        PyObjectPayload::Tuple(items) => items
                            .iter()
                            .map(|item| match &item.payload {
                                PyObjectPayload::Cell(cell) => cell.clone(),
                                _ => Rc::new(PyCell::new(Some(item.clone()))),
                            })
                            .collect(),
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                };
                let mut annotations = IndexMap::new();
                if flags & 0x04 != 0 {
                    let ann_obj = frame.pop();
                    if let PyObjectPayload::Dict(m) = &ann_obj.payload {
                        for (k, v) in m.read().iter() {
                            if let HashableKey::Str(name) = k {
                                annotations.insert(name.to_compact_string(), v.clone());
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
                                result.insert(name.to_compact_string(), v.clone());
                            }
                        }
                        result
                    } else {
                        IndexMap::new()
                    }
                } else {
                    IndexMap::new()
                };
                let mut defaults = Vec::new();
                if flags & 0x01 != 0 {
                    let default_tuple = frame.pop();
                    defaults = default_tuple.to_list().unwrap_or_default();
                }
                let code: Rc<CodeObject> = match &code_obj.payload {
                    PyObjectPayload::Code(c) => Rc::clone(c),
                    _ => {
                        return Err(PyException::type_error(
                            "expected code object for MAKE_FUNCTION",
                        ))
                    }
                };
                let qualname_str = qualname
                    .as_str()
                    .map(CompactString::from)
                    .unwrap_or_else(|| code.name.clone());
                let constant_cache = PyFunction::get_or_build_constant_cache(&code);
                let is_simple = PyFunction::compute_is_simple_static(&code, &closure_cells);
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
                    attrs: Rc::new(PyCell::new(FxAttrMap::default())),
                    is_simple,
                };
                frame.push(PyObject::function(func));
            }
            // Fallback: decompose LoadGlobalCallFunction into LoadGlobal + CallFunction
            Opcode::LoadGlobalCallFunction => {
                let name_idx = (instr.arg >> 16) as usize;
                let arg_count = (instr.arg & 0xFFFF) as u32;
                let load_instr = Instruction::new(Opcode::LoadGlobal, name_idx as u32);
                self.exec_name_ops(load_instr)?;
                let call_instr = Instruction::new(Opcode::CallFunction, arg_count);
                return self.exec_call_ops(call_instr);
            }
            // Fallback: decompose LoadFastLoadAttr into LoadFast + LoadAttr
            Opcode::LoadFastLoadAttr => {
                let local_idx = (instr.arg >> 16) as usize;
                let name_idx = (instr.arg & 0xFFFF) as u32;
                let load_instr = Instruction::new(Opcode::LoadFast, local_idx as u32);
                self.exec_name_ops(load_instr)?;
                let attr_instr = Instruction::new(Opcode::LoadAttr, name_idx);
                return self.exec_attr_ops(attr_instr);
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
