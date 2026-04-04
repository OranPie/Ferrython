//! Opcode group handlers for the VM.
//!
//! This module splits the monolithic `execute_one` match into logically
//! grouped methods, each handling a family of related opcodes.

use crate::builtins;
use crate::frame::{BlockKind, Frame, ScopeKind};
use crate::vm::exception_kind_matches;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::{CodeObject, Instruction};
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, CompareOp, IteratorData,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyFunction, PyInt};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Unwrap IntEnum/IntFlag members to their `_value_` for arithmetic operations.
fn unwrap_int_enum(obj: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(value) = inst.attrs.read().get("_value_") {
            // Check if this is an enum member (has _value_ and _name_)
            if inst.attrs.read().contains_key("_name_") {
                return value.clone();
            }
        }
    }
    obj.clone()
}

/// Helpers: stack access without holding a long-lived frame borrow.
impl VirtualMachine {
    #[inline]
    pub(crate) fn vm_push(&mut self, val: PyObjectRef) {
        self.call_stack.last_mut().unwrap().push(val);
    }
    #[inline]
    pub(crate) fn vm_pop(&mut self) -> PyObjectRef {
        self.call_stack.last_mut().unwrap().pop()
    }
    #[inline]
    pub(crate) fn vm_pop2(&mut self) -> (PyObjectRef, PyObjectRef) {
        let f = self.call_stack.last_mut().unwrap();
        let b = f.pop();
        let a = f.pop();
        (a, b)
    }
    #[inline]
    pub(crate) fn vm_frame(&mut self) -> &mut Frame {
        self.call_stack.last_mut().unwrap()
    }
}

/// Check if a class has a user-defined method override (in its own namespace, not inherited).
impl VirtualMachine {
    pub(crate) fn class_has_user_override(cls: &PyObjectRef, method_name: &str) -> bool {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if let Some(v) = cd.namespace.read().get(method_name) {
                return matches!(&v.payload, PyObjectPayload::Function(_));
            }
        }
        false
    }
}

// ── Group 1: Stack + LoadConst ───────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_stack_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        match instr.op {
            Opcode::Nop => {}
            Opcode::PopTop => { frame.pop(); }
            Opcode::RotTwo => {
                let a = frame.pop();
                let b = frame.pop();
                frame.push(a);
                frame.push(b);
            }
            Opcode::RotThree => {
                let a = frame.pop();
                let b = frame.pop();
                let c = frame.pop();
                frame.push(a);
                frame.push(c);
                frame.push(b);
            }
            Opcode::RotFour => {
                let a = frame.pop();
                let b = frame.pop();
                let c = frame.pop();
                let d = frame.pop();
                frame.push(a);
                frame.push(d);
                frame.push(c);
                frame.push(b);
            }
            Opcode::DupTop => {
                let v = frame.peek().clone();
                frame.push(v);
            }
            Opcode::DupTopTwo => {
                let top = frame.stack[frame.stack.len() - 1].clone();
                let second = frame.stack[frame.stack.len() - 2].clone();
                frame.push(second);
                frame.push(top);
            }
            Opcode::LoadConst => {
                let idx = instr.arg as usize;
                let obj = frame.constant_cache[idx].clone();
                frame.push(obj);
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

// ── Group 2: Name loading/storing ────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_name_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let mut del_target: Option<PyObjectRef> = None;
        {
        let frame = self.vm_frame();
        match instr.op {
            Opcode::LoadName => {
                let name = &frame.code.names[instr.arg as usize];
                match frame.load_name(name) {
                    Some(v) => frame.push(v),
                    None => return Err(PyException::name_error(format!(
                        "name '{}' is not defined", name
                    ))),
                }
            }
            Opcode::StoreName => {
                let name = frame.code.names[instr.arg as usize].clone();
                let value = frame.pop();
                match frame.scope_kind {
                    ScopeKind::Module => { frame.globals.write().insert(name, value); }
                    ScopeKind::Class => { frame.local_names.insert(name, value); }
                    ScopeKind::Function => { frame.local_names.insert(name, value); }
                }
            }
            Opcode::DeleteName => {
                let name = frame.code.names[instr.arg as usize].clone();
                let old = frame.local_names.get(name.as_str()).cloned()
                    .or_else(|| frame.globals.read().get(name.as_str()).cloned());
                frame.local_names.shift_remove(name.as_str());
                frame.globals.write().shift_remove(name.as_str());
                if let Some(ref obj) = old {
                    if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
                        if obj.get_attr("__del__").is_some() {
                            del_target = old;
                        }
                    }
                }
            }
            Opcode::LoadFast => {
                let idx = instr.arg as usize;
                match frame.get_local(idx) {
                    Some(v) => {
                        let v = v.clone();
                        frame.push(v);
                    }
                    None => {
                        let name = &frame.code.varnames[idx];
                        return Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment", name
                        )));
                    }
                }
            }
            Opcode::StoreFast => {
                let value = frame.pop();
                frame.set_local(instr.arg as usize, value);
            }
            Opcode::DeleteFast => {
                let idx = instr.arg as usize;
                frame.locals[idx] = None;
            }
            Opcode::LoadDeref => {
                let idx = instr.arg as usize;
                let val = frame.cells[idx].read().clone();
                match val {
                    Some(v) => { frame.push(v); }
                    None => {
                        let n_cell = frame.code.cellvars.len();
                        let name = if idx < n_cell {
                            frame.code.cellvars[idx].clone()
                        } else {
                            frame.code.freevars[idx - n_cell].clone()
                        };
                        return Err(PyException::name_error(format!(
                            "free variable '{}' referenced before assignment in enclosing scope", name
                        )));
                    }
                }
            }
            Opcode::StoreDeref => {
                let value = frame.pop();
                let idx = instr.arg as usize;
                *frame.cells[idx].write() = Some(value);
            }
            Opcode::DeleteDeref => {
                let idx = instr.arg as usize;
                *frame.cells[idx].write() = None;
            }
            Opcode::LoadClassderef => {
                // Like LoadDeref but checks locals first (for class scoping).
                let idx = instr.arg as usize;
                let n_cell = frame.code.cellvars.len();
                let name = if idx < n_cell {
                    frame.code.cellvars[idx].clone()
                } else {
                    frame.code.freevars[idx - n_cell].clone()
                };
                // Check local_names first (class namespace)
                if let Some(v) = frame.local_names.get(name.as_str()).cloned() {
                    frame.push(v);
                } else {
                    // Fall back to cell
                    let val = frame.cells[idx].read().clone();
                    match val {
                        Some(v) => { frame.push(v); }
                        None => {
                            return Err(PyException::name_error(format!(
                                "free variable '{}' referenced before assignment in enclosing scope", name
                            )));
                        }
                    }
                }
            }
            Opcode::LoadClosure => {
                let idx = instr.arg as usize;
                let cell = frame.cells[idx].clone();
                frame.push(PyObject::cell(cell));
            }
            Opcode::LoadGlobal => {
                let name = &frame.code.names[instr.arg as usize];
                let from_globals = frame.globals.read().get(name.as_str()).cloned();
                if let Some(v) = from_globals {
                    frame.push(v);
                } else if let Some(v) = frame.builtins.get(name.as_str()) {
                    let v = v.clone();
                    frame.push(v);
                } else {
                    return Err(PyException::name_error(format!(
                        "name '{}' is not defined", name
                    )));
                }
            }
            Opcode::StoreGlobal => {
                let name = frame.code.names[instr.arg as usize].clone();
                let value = frame.pop();
                frame.globals.write().insert(name, value);
            }
            Opcode::DeleteGlobal => {
                let name = &frame.code.names[instr.arg as usize];
                frame.globals.write().shift_remove(name.as_str());
            }
            _ => unreachable!(),
        }
        } // drop frame borrow
        // Call __del__ if we captured a deleted object with __del__
        if let Some(obj) = del_target {
            if let Some(del_fn) = obj.get_attr("__del__") {
                let _ = self.call_object(del_fn, vec![]);
            }
        }
        Ok(None)
    }
}

// ── Group 3: Attribute operations ────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_attr_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        let name = frame.code.names[instr.arg as usize].clone();
        match instr.op {
            Opcode::LoadAttr => {
                let obj = self.vm_pop();
                self.exec_load_attr(&name, obj)
            }
            Opcode::StoreAttr => {
                let obj = self.vm_pop();
                let value = self.vm_pop();
                self.exec_store_attr(&name, obj, value)
            }
            Opcode::DeleteAttr => {
                let obj = self.vm_pop();
                self.exec_delete_attr(&name, obj)
            }
            _ => unreachable!(),
        }
    }

    fn exec_load_attr(&mut self, name: &CompactString, obj: PyObjectRef) -> Result<Option<PyObjectRef>, PyException> {
        // __getattribute__ override: called before normal lookup
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(ga) = lookup_in_class_mro(&inst.class, "__getattribute__") {
                if matches!(&ga.payload, PyObjectPayload::Function(_)) {
                    let method = Arc::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: obj.clone(),
                            method: ga,
                        }
                    });
                    let name_arg = PyObject::str_val(CompactString::from(name.as_str()));
                    match self.call_object(method, vec![name_arg]) {
                        Ok(result) => {
                            self.vm_push(result);
                            return Ok(None);
                        }
                        Err(e) if e.kind == ExceptionKind::AttributeError => {
                            // Fall through to __getattr__
                            if let Some(ga2) = obj.get_attr("__getattr__") {
                                let name_arg2 = PyObject::str_val(CompactString::from(name.as_str()));
                                let result = self.call_object(ga2, vec![name_arg2])?;
                                self.vm_push(result);
                                return Ok(None);
                            }
                            return Err(e);
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        match obj.get_attr(name) {
            Some(v) => {
                if let PyObjectPayload::Property { fget, .. } = &v.payload {
                    if let Some(getter) = fget {
                        let getter = getter.clone();
                        let result = self.call_object(getter, vec![obj])?;
                        self.vm_push(result);
                    } else {
                        return Err(PyException::attribute_error(format!(
                            "unreadable attribute '{}'", name
                        )));
                    }
                } else if v.get_attr("__cached_property_func__").is_some() {
                    // cached_property: compute once, cache in instance dict
                    let func = if let PyObjectPayload::Instance(ref cp_inst) = v.payload {
                        cp_inst.attrs.read().get("__cached_property_func__").cloned()
                    } else { None };
                    if let Some(func) = func {
                        let result = self.call_object(func, vec![obj.clone()])?;
                        if let PyObjectPayload::Instance(ref inst) = obj.payload {
                            inst.attrs.write().insert(CompactString::from(name.as_str()), result.clone());
                        }
                        self.vm_push(result);
                    } else {
                        self.vm_push(v);
                    }
                } else if has_descriptor_get(&v) {
                    // Custom descriptor protocol: call __get__(self, instance, owner)
                    let get_method = v.get_attr("__get__").unwrap();
                    let (instance_arg, owner_arg) = match &obj.payload {
                        PyObjectPayload::Instance(inst) => (obj.clone(), inst.class.clone()),
                        PyObjectPayload::Class(_) => (PyObject::none(), obj.clone()),
                        _ => (obj.clone(), PyObject::none()),
                    };
                    let get_method_bound = if matches!(&get_method.payload, PyObjectPayload::BoundMethod { .. }) {
                        get_method
                    } else {
                        Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: v.clone(),
                                method: get_method,
                            }
                        })
                    };
                    let result = self.call_object(get_method_bound, vec![instance_arg, owner_arg])?;
                    self.vm_push(result);
                } else if matches!(&obj.payload, PyObjectPayload::Module(_))
                    && matches!(&v.payload, PyObjectPayload::NativeFunction { .. })
                    && obj.get_attr("_bind_methods").is_some()
                {
                    self.vm_push(Arc::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: obj,
                            method: v,
                        }
                    }));
                } else {
                    self.vm_push(v);
                }
            }
            None => {
                // Type method access: e.g., dict.fromkeys, object.__getattribute__
                let type_name = match &obj.payload {
                    PyObjectPayload::NativeFunction { name: fn_name, .. } => Some(fn_name.as_str()),
                    PyObjectPayload::BuiltinType(tn) => Some(tn.as_str()),
                    PyObjectPayload::BuiltinFunction(fn_name) => Some(fn_name.as_str()),
                    _ => None,
                };
                if let Some(tn) = type_name {
                    if let Some(type_method) = builtins::resolve_type_class_method(tn, name) {
                        self.vm_push(type_method);
                        return Ok(None);
                    }
                }
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
        Ok(None)
    }

    fn exec_store_attr(&mut self, name: &CompactString, obj: PyObjectRef, value: PyObjectRef) -> Result<Option<PyObjectRef>, PyException> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(desc) = lookup_in_class_mro(&inst.class, name) {
                if let PyObjectPayload::Property { fset, .. } = &desc.payload {
                    if let Some(setter) = fset {
                        let setter = setter.clone();
                        self.call_object(setter, vec![obj, value])?;
                        return Ok(None);
                    } else {
                        return Err(PyException::attribute_error(format!(
                            "can't set attribute '{}'", name
                        )));
                    }
                }
                // Custom data descriptor with __set__
                if is_data_descriptor(&desc) {
                    if let Some(set_method) = desc.get_attr("__set__") {
                        let set_bound = if matches!(&set_method.payload, PyObjectPayload::BoundMethod { .. }) {
                            set_method
                        } else {
                            Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: desc,
                                    method: set_method,
                                }
                            })
                        };
                        self.call_object(set_bound, vec![obj, value])?;
                        return Ok(None);
                    }
                }
            }
        }
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                match &sa.payload {
                    PyObjectPayload::Function(_) => {
                        let method = Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: sa,
                            }
                        });
                        let name_arg = PyObject::str_val(name.clone());
                        self.call_object(method, vec![name_arg, value])?;
                        return Ok(None);
                    }
                    PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } => {
                        let name_arg = PyObject::str_val(name.clone());
                        self.call_object(sa, vec![obj, name_arg, value])?;
                        return Ok(None);
                    }
                    _ => {}
                }
            }
        }
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Check __slots__ restriction — accumulate from entire MRO
                let has_slots = {
                    let mut all_slots: Vec<String> = Vec::new();
                    let mut found_any = false;
                    // Collect __slots__ from the class and all bases in MRO
                    let classes_to_check: Vec<PyObjectRef> = {
                        let mut v = vec![inst.class.clone()];
                        if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            v.extend(cd.mro.clone());
                            v.extend(cd.bases.clone());
                        }
                        v
                    };
                    for cls in &classes_to_check {
                        if let PyObjectPayload::Class(cd) = &cls.payload {
                            if let Some(slots) = cd.namespace.read().get("__slots__").cloned() {
                                if matches!(&slots.payload, PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)) {
                                    found_any = true;
                                    if let Ok(items) = slots.to_list() {
                                        for item in &items {
                                            let s = item.py_to_string();
                                            if !all_slots.contains(&s) {
                                                all_slots.push(s);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if found_any { Some(all_slots) } else { None }
                };
                if let Some(allowed_names) = has_slots {
                    // If __dict__ is in slots, allow any attribute
                    if !allowed_names.iter().any(|s| s == "__dict__") {
                        if !allowed_names.iter().any(|s| s == name.as_str()) {
                            return Err(PyException::attribute_error(format!(
                                "'{}' object has no attribute '{}'",
                                inst.class.get_attr("__name__")
                                    .map(|n| n.py_to_string())
                                    .unwrap_or_else(|| "object".to_string()),
                                name
                            )));
                        }
                    }
                }
                inst.attrs.write().insert(name.clone(), value);
            }
            PyObjectPayload::Class(cd) => {
                cd.namespace.write().insert(name.clone(), value);
                cd.invalidate_cache();
            }
            PyObjectPayload::Module(md) => {
                md.attrs.write().insert(name.clone(), value);
            }
            PyObjectPayload::Function(f) => {
                f.attrs.write().insert(name.clone(), value);
            }
            _ => {
                return Err(PyException::attribute_error(format!(
                    "'{}' object does not support attribute assignment", obj.type_name()
                )));
            }
        }
        Ok(None)
    }

    fn exec_delete_attr(&mut self, name: &CompactString, obj: PyObjectRef) -> Result<Option<PyObjectRef>, PyException> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Check for descriptor with __delete__ or property fdel first
                if let Some(class_attr) = lookup_in_class_mro(&inst.class, name.as_str()) {
                    if let PyObjectPayload::Property { fdel, .. } = &class_attr.payload {
                        if let Some(fdel_fn) = fdel {
                            let bound = Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: fdel_fn.clone(),
                                }
                            });
                            self.call_object(bound, vec![])?;
                        } else {
                            return Err(PyException::attribute_error(format!(
                                "can't delete attribute '{}'", name)));
                        }
                    } else if is_data_descriptor(&class_attr) {
                        // Data descriptor with __delete__
                        if let Some(del_method) = class_attr.get_attr("__delete__") {
                            let del_bound = if matches!(&del_method.payload, PyObjectPayload::BoundMethod { .. }) {
                                del_method
                            } else {
                                Arc::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: class_attr.clone(),
                                        method: del_method,
                                    }
                                })
                            };
                            self.call_object(del_bound, vec![obj.clone()])?;
                        } else {
                            if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'", obj.type_name(), name)));
                            }
                        }
                    } else if let Some(delattr_method) = lookup_in_class_mro(&inst.class, "__delattr__") {
                        if matches!(&delattr_method.payload, PyObjectPayload::Function(_)) {
                            let method = Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod { receiver: obj.clone(), method: delattr_method }
                            });
                            let name_arg = PyObject::str_val(name.clone());
                            self.call_object(method, vec![name_arg])?;
                        } else {
                            if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'", obj.type_name(), name)));
                            }
                        }
                    } else {
                        if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                            return Err(PyException::attribute_error(format!(
                                "'{}' object has no attribute '{}'", obj.type_name(), name)));
                        }
                    }
                } else if let Some(delattr_method) = lookup_in_class_mro(&inst.class, "__delattr__") {
                    if matches!(&delattr_method.payload, PyObjectPayload::Function(_)) {
                        let method = Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod { receiver: obj.clone(), method: delattr_method }
                        });
                        let name_arg = PyObject::str_val(name.clone());
                        self.call_object(method, vec![name_arg])?;
                    } else {
                        if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                            return Err(PyException::attribute_error(format!(
                                "'{}' object has no attribute '{}'", obj.type_name(), name)));
                        }
                    }
                } else {
                    if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                        return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), name)));
                    }
                }
            }
            PyObjectPayload::Class(cd) => {
                if cd.namespace.write().swap_remove(name.as_str()).is_none() {
                    return Err(PyException::attribute_error(format!(
                        "type object has no attribute '{}'", name)));
                }
                cd.invalidate_cache();
            }
            _ => return Err(PyException::attribute_error(format!(
                "'{}' object does not support attribute deletion", obj.type_name()))),
        }
        Ok(None)
    }
}

// ── Group 4: Unary operations ────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_unary_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::UnaryPositive => {
                let v = self.vm_pop();
                if let Some(r) = self.try_call_dunder(&v, "__pos__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.positive()?);
                }
            }
            Opcode::UnaryNegative => {
                let v = self.vm_pop();
                if let Some(r) = self.try_call_dunder(&v, "__neg__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.negate()?);
                }
            }
            Opcode::UnaryNot => {
                let v = self.vm_pop();
                let truthy = self.vm_is_truthy(&v)?;
                self.vm_push(PyObject::bool_val(!truthy));
            }
            Opcode::UnaryInvert => {
                let v = self.vm_pop();
                if let Some(r) = self.try_call_dunder(&v, "__invert__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.invert()?);
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

// ── Group 5: Binary / inplace arithmetic ─────────────────────────────
impl VirtualMachine {
    pub(crate) fn try_binary_dunder(
        &mut self, a: &PyObjectRef, b: &PyObjectRef,
        dunder: &str, rdunder: Option<&str>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        // Look up dunder via class MRO (not instance get_attr) for proper inheritance
        if let PyObjectPayload::Instance(inst) = &a.payload {
            if let Some(method) = lookup_in_class_mro(&inst.class, dunder) {
                let bound = self.bind_method(a, method);
                let result = self.call_object(bound, vec![b.clone()])?;
                // If method returns NotImplemented, try the reflected dunder
                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Ok(Some(result));
                }
            }
        }
        if let Some(rd) = rdunder {
            if let PyObjectPayload::Instance(inst) = &b.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, rd) {
                    let bound = self.bind_method(b, method);
                    let result = self.call_object(bound, vec![a.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(result));
                    }
                }
            }
        }
        Ok(None)
    }

    fn try_inplace_dunder(
        &mut self, a: &PyObjectRef, b: &PyObjectRef,
        idunder: &str, dunder: &str,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if let PyObjectPayload::Instance(inst) = &a.payload {
            let method = lookup_in_class_mro(&inst.class, idunder)
                .or_else(|| lookup_in_class_mro(&inst.class, dunder));
            if let Some(m) = method {
                let bound = self.bind_method(a, m);
                return Ok(Some(self.call_object(bound, vec![b.clone()])?));
            }
        }
        Ok(None)
    }

    /// Create a bound method from an instance receiver and an unbound method.
    fn bind_method(&self, receiver: &PyObjectRef, method: PyObjectRef) -> PyObjectRef {
        match &method.payload {
            PyObjectPayload::BoundMethod { .. } => method,
            _ => Arc::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: receiver.clone(),
                    method,
                }
            }),
        }
    }

    pub(crate) fn exec_binary_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let (a, b) = self.vm_pop2();

        // ── Fast paths for primitive types ──
        // Skip dunder dispatch and py_add/sub/mul overhead for the most common cases.
        // Only applies to BinaryAdd/Sub/Mul — the hottest arithmetic opcodes.
        macro_rules! fast_int_op {
            ($a:expr, $b:expr, $checked_op:ident, $big_op:tt) => {
                match (&$a.payload, &$b.payload) {
                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                        let result = match x.$checked_op(*y) {
                            Some(r) => PyObject::int(r),
                            None => {
                                use num_bigint::BigInt;
                                PyObject::big_int(BigInt::from(*x) $big_op BigInt::from(*y))
                            }
                        };
                        self.vm_push(result);
                        return Ok(None);
                    }
                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                        self.vm_push(PyObject::float(*x $big_op *y));
                        return Ok(None);
                    }
                    _ => {}
                }
            };
        }

        match instr.op {
            Opcode::BinaryAdd | Opcode::InplaceAdd => {
                fast_int_op!(a, b, checked_add, +);
                // Also fast-path str + str
                if let (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) = (&a.payload, &b.payload) {
                    let mut s = String::with_capacity(x.len() + y.len());
                    s.push_str(x.as_str());
                    s.push_str(y.as_str());
                    self.vm_push(PyObject::str_val(CompactString::from(s)));
                    return Ok(None);
                }
            }
            Opcode::BinarySubtract | Opcode::InplaceSubtract => {
                fast_int_op!(a, b, checked_sub, -);
            }
            Opcode::BinaryMultiply | Opcode::InplaceMultiply => {
                fast_int_op!(a, b, checked_mul, *);
            }
            _ => {}
        }

        // ── Standard path: dunder dispatch + fallback ──
        // For IntEnum/IntFlag members, if the primitive op fails, retry with _value_
        macro_rules! with_enum_fallback {
            ($a:expr, $b:expr, $op:ident) => {
                match $a.$op(&$b) {
                    Ok(r) => r,
                    Err(_) => {
                        let ua = unwrap_int_enum(&$a);
                        let ub = unwrap_int_enum(&$b);
                        if !Arc::ptr_eq(&ua, &$a) || !Arc::ptr_eq(&ub, &$b) {
                            ua.$op(&ub)?
                        } else {
                            return Err($a.$op(&$b).unwrap_err());
                        }
                    }
                }
            };
        }
        let result = match instr.op {
            Opcode::BinaryAdd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__add__", Some("__radd__"))? { r }
                else { with_enum_fallback!(a, b, add) }
            }
            Opcode::InplaceAdd => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__iadd__", "__add__")? { r }
                else { with_enum_fallback!(a, b, add) }
            }
            Opcode::BinarySubtract => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? { r }
                else { with_enum_fallback!(a, b, sub) }
            }
            Opcode::InplaceSubtract => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__isub__", "__sub__")? { r }
                else { with_enum_fallback!(a, b, sub) }
            }
            Opcode::BinaryMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mul__", Some("__rmul__"))? { r }
                else { with_enum_fallback!(a, b, mul) }
            }
            Opcode::InplaceMultiply => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imul__", "__mul__")? { r }
                else { with_enum_fallback!(a, b, mul) }
            }
            Opcode::BinaryTrueDivide => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__truediv__", Some("__rtruediv__"))? { r }
                else { with_enum_fallback!(a, b, true_div) }
            }
            Opcode::InplaceTrueDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__itruediv__", "__truediv__")? { r }
                else { with_enum_fallback!(a, b, true_div) }
            }
            Opcode::BinaryFloorDivide => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__floordiv__", Some("__rfloordiv__"))? { r }
                else { with_enum_fallback!(a, b, floor_div) }
            }
            Opcode::InplaceFloorDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ifloordiv__", "__floordiv__")? { r }
                else { with_enum_fallback!(a, b, floor_div) }
            }
            Opcode::BinaryModulo | Opcode::InplaceModulo => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mod__", Some("__rmod__"))? { r }
                else { with_enum_fallback!(a, b, modulo) }
            }
            Opcode::BinaryPower | Opcode::InplacePower => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__pow__", Some("__rpow__"))? { r }
                else { with_enum_fallback!(a, b, power) }
            }
            Opcode::BinaryLshift | Opcode::InplaceLshift => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__lshift__", Some("__rlshift__"))? { r }
                else { with_enum_fallback!(a, b, lshift) }
            }
            Opcode::BinaryRshift | Opcode::InplaceRshift => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__rshift__", Some("__rrshift__"))? { r }
                else { with_enum_fallback!(a, b, rshift) }
            }
            Opcode::BinaryAnd | Opcode::InplaceAnd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__and__", Some("__rand__"))? { r }
                else { with_enum_fallback!(a, b, bit_and) }
            }
            Opcode::BinaryOr | Opcode::InplaceOr => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__or__", Some("__ror__"))? { r }
                else { with_enum_fallback!(a, b, bit_or) }
            }
            Opcode::BinaryXor | Opcode::InplaceXor => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__xor__", Some("__rxor__"))? { r }
                else { with_enum_fallback!(a, b, bit_xor) }
            }
            Opcode::BinaryMatrixMultiply | Opcode::InplaceMatrixMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__matmul__", Some("__rmatmul__"))? { r }
                else {
                    return Err(PyException::type_error(format!(
                        "unsupported operand type(s) for @: '{}' and '{}'",
                        a.type_name(), b.type_name()
                    )));
                }
            }
            _ => unreachable!(),
        };
        self.vm_push(result);
        Ok(None)
    }

}

// ── Group 6: Subscript operations ────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_subscript_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::BinarySubscr => {
                let raw_key = self.vm_pop();
                let obj = self.vm_pop();
                // Resolve __index__ on the key if it's an Instance with __index__
                let key = if matches!(&raw_key.payload, PyObjectPayload::Instance(_)) {
                    if let Some(r) = self.try_call_dunder(&raw_key, "__index__", vec![])? {
                        r
                    } else {
                        raw_key
                    }
                } else {
                    raw_key
                };
                // __class_getitem__: MyClass[int] → MyClass.__class_getitem__(cls, int)
                if matches!(&obj.payload, PyObjectPayload::Class(_)) {
                    if let Some(cgi) = obj.get_attr("__class_getitem__") {
                        let result = self.call_object(cgi, vec![obj.clone(), key])?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                    // Enum-style __getitem__: Color["RED"]
                    if let Some(gi) = obj.get_attr("__getitem__") {
                        let result = self.call_object(gi, vec![obj.clone(), key])?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                    // typing generic alias: List[int] → _GenericAlias with str
                    if let Some(typing_name) = obj.get_attr("__typing_name__") {
                        let name = typing_name.py_to_string();
                        let params = match &key.payload {
                            PyObjectPayload::Tuple(items) => {
                                items.iter().map(|i| i.type_name().to_string()).collect::<Vec<_>>().join(", ")
                            }
                            _ => key.type_name().to_string(),
                        };
                        let repr = format!("{}[{}]", name, params);
                        let alias_cls = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                        let mut attrs = IndexMap::new();
                        attrs.insert(CompactString::from("__typing_repr__"), PyObject::str_val(CompactString::from(&repr)));
                        attrs.insert(CompactString::from("__str__"), PyObject::str_val(CompactString::from(&repr)));
                        self.vm_push(PyObject::instance_with_attrs(alias_cls, attrs));
                        return Ok(None);
                    }
                    // Generic fallback: Class[X] returns the class itself (PEP 585)
                    self.vm_push(obj.clone());
                    return Ok(None);
                }
                // BuiltinType subscript: list[int], dict[str, int] → returns the type (PEP 585)
                if matches!(&obj.payload, PyObjectPayload::BuiltinType(_)) {
                    self.vm_push(obj.clone());
                    return Ok(None);
                }
                // Dict subclass: Instance with dict_storage
                // If the subclass defines its own __getitem__, call it instead of dict_storage
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(ref ds) = inst.dict_storage {
                        let has_user_getitem = Self::class_has_user_override(&inst.class, "__getitem__");
                        if has_user_getitem {
                            // Let dunder dispatch handle it below
                        } else {
                            let hk = self.vm_to_hashable_key(&key)?;
                            let existing = ds.read().get(&hk).cloned();
                            if let Some(val) = existing {
                                self.vm_push(val);
                            } else {
                                // Check for __missing__
                                if let Some(missing) = obj.get_attr("__missing__") {
                                    let result = self.call_object(missing, vec![key])?;
                                    self.vm_push(result);
                                } else {
                                    return Err(PyException::key_error(key.py_to_string()));
                                }
                            }
                            return Ok(None);
                        }
                    }
                }
                if let Some(r) = self.try_call_dunder(&obj, "__getitem__", vec![key.clone()])? {
                    self.vm_push(r);
                    return Ok(None);
                }
                if let PyObjectPayload::Instance(_) = &obj.payload {
                    if let Some(tup) = obj.get_attr("_tuple") {
                        self.vm_push(tup.get_item(&key)?);
                        return Ok(None);
                    }
                }
                if let PyObjectPayload::Dict(map) = &obj.payload {
                    let hk = self.vm_to_hashable_key(&key)?;
                    let existing = map.read().get(&hk).cloned();
                    if let Some(val) = existing {
                        self.vm_push(val);
                    } else {
                        let factory_key = HashableKey::Str(CompactString::from("__defaultdict_factory__"));
                        let factory = map.read().get(&factory_key).cloned();
                        if let Some(factory) = factory {
                            let default = self.call_object(factory, vec![])?;
                            map.write().insert(hk, default.clone());
                            self.vm_push(default);
                            return Ok(None);
                        } else {
                            return Err(PyException::key_error(key.py_to_string()));
                        }
                    }
                } else {
                    self.vm_push(obj.get_item(&key)?);
                }
            }
            Opcode::StoreSubscr => {
                let key = self.vm_pop();
                let obj = self.vm_pop();
                let value = self.vm_pop();
                match &obj.payload {
                    PyObjectPayload::List(items) => {
                        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
                            let step_val = step.as_ref().map(|v| v.as_int().unwrap_or(1)).unwrap_or(1);
                            let new_items = value.to_list()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            
                            if step_val == 1 || step_val == 0 {
                                // Contiguous slice assignment: a[s:e] = items
                                let s_val = start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0);
                                let e_val = stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len);
                                let s = (if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) }) as usize;
                                let e = (if e_val < 0 { (len + e_val).max(0) } else { e_val.min(len) }) as usize;
                                let e = e.max(s);
                                w.splice(s..e, new_items);
                            } else {
                                // Extended slice assignment: a[s:e:step] = items
                                let s_val = if step_val > 0 {
                                    start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0)
                                } else {
                                    start.as_ref().map(|v| v.as_int().unwrap_or(len - 1)).unwrap_or(len - 1)
                                };
                                let e_val = if step_val > 0 {
                                    stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len)
                                } else {
                                    stop.as_ref().map(|v| v.as_int().unwrap_or(-len - 1)).unwrap_or(-len - 1)
                                };
                                // Collect indices
                                let mut indices = Vec::new();
                                let mut i = if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) };
                                let end = if e_val < 0 { (len + e_val).max(-1) } else { e_val.min(len) };
                                if step_val > 0 {
                                    while i < end {
                                        indices.push(i as usize);
                                        i += step_val;
                                    }
                                } else {
                                    while i > end {
                                        indices.push(i as usize);
                                        i += step_val;
                                    }
                                }
                                if indices.len() != new_items.len() {
                                    return Err(PyException::value_error(format!(
                                        "attempt to assign sequence of size {} to extended slice of size {}",
                                        new_items.len(), indices.len()
                                    )));
                                }
                                for (idx, val) in indices.iter().zip(new_items.iter()) {
                                    w[*idx] = val.clone();
                                }
                            }
                        } else {
                            let idx = key.to_int()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("list assignment index out of range"));
                            }
                            w[actual as usize] = value;
                        }
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = self.vm_to_hashable_key(&key)?;
                        map.write().insert(hk, value);
                    }
                    PyObjectPayload::ByteArray(ref bytes) => {
                        let idx = key.to_int()?;
                        let byte_val = value.to_int()? as u8;
                        // ByteArray is immutable-shared via Arc, so we need unsafe or a wrapper.
                        // Actually, ByteArray uses Vec<u8> directly in the payload.
                        // We need a mutable reference. Since PyObjectPayload::ByteArray wraps Vec<u8>,
                        // we can't mutate through Arc. Let's handle this via a workaround.
                        let len = bytes.len() as i64;
                        let actual = if idx < 0 { len + idx } else { idx };
                        if actual < 0 || actual >= len {
                            return Err(PyException::index_error("bytearray index out of range"));
                        }
                        // Use unsafe to mutate the inner bytes through the Arc
                        unsafe {
                            let ptr = bytes.as_ptr() as *mut u8;
                            *ptr.add(actual as usize) = byte_val;
                        }
                    }
                    PyObjectPayload::InstanceDict(attrs) => {
                        let key_str = CompactString::from(key.py_to_string());
                        attrs.write().insert(key_str, value);
                    }
                    PyObjectPayload::Instance(inst) => {
                        // Dict subclass: use dict_storage if no user override
                        if let Some(ref ds) = inst.dict_storage {
                            let has_user_setitem = Self::class_has_user_override(&inst.class, "__setitem__");
                            if has_user_setitem {
                                if let Some(m) = obj.get_attr("__setitem__") {
                                    self.call_object(m, vec![key, value])?;
                                    return Ok(None);
                                }
                            }
                            let hk = self.vm_to_hashable_key(&key)?;
                            ds.write().insert(hk, value);
                        } else if let Some(m) = obj.get_attr("__setitem__") {
                            self.call_object(m, vec![key, value])?;
                            return Ok(None);
                        } else {
                            return Err(PyException::type_error(format!(
                                "'{}' object does not support item assignment", obj.type_name())));
                        }
                    }
                    _ => return Err(PyException::type_error(format!(
                        "'{}' object does not support item assignment", obj.type_name()))),
                }
            }
            Opcode::DeleteSubscr => {
                let key = self.vm_pop();
                let obj = self.vm_pop();
                match &obj.payload {
                    PyObjectPayload::List(items) => {
                        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let s = start.as_ref().map(|v| v.to_int().unwrap_or(0)).unwrap_or(0);
                            let e = stop.as_ref().map(|v| v.to_int().unwrap_or(len)).unwrap_or(len);
                            let st = step.as_ref().map(|v| v.to_int().unwrap_or(1)).unwrap_or(1);
                            let s = if s < 0 { (len + s).max(0) } else { s.min(len) };
                            let e = if e < 0 { (len + e).max(0) } else { e.min(len) };
                            if st == 1 && s <= e {
                                w.drain(s as usize..e as usize);
                            } else if st == -1 && s >= e {
                                let mut indices: Vec<usize> = ((e + 1) as usize..=(s) as usize).collect();
                                indices.reverse();
                                for idx in indices {
                                    if idx < w.len() { w.remove(idx); }
                                }
                            } else if st > 1 {
                                let mut indices = Vec::new();
                                let mut i = s;
                                while i < e { indices.push(i as usize); i += st; }
                                indices.reverse();
                                for idx in indices { if idx < w.len() { w.remove(idx); } }
                            }
                        } else {
                            let idx = key.to_int()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("list assignment index out of range"));
                            }
                            w.remove(actual as usize);
                        }
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = self.vm_to_hashable_key(&key)?;
                        if map.write().shift_remove(&hk).is_none() {
                            return Err(PyException::key_error(key.repr()));
                        }
                    }
                    PyObjectPayload::Instance(inst) => {
                        if let Some(method) = obj.get_attr("__delitem__") {
                            self.call_object(method, vec![key])?;
                            return Ok(None);
                        }
                        if let Some(ref ds) = inst.dict_storage {
                            let hk = self.vm_to_hashable_key(&key)?;
                            if ds.write().shift_remove(&hk).is_none() {
                                return Err(PyException::key_error(key.repr()));
                            }
                            return Ok(None);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}'  object does not support item deletion", obj.type_name())));
                    }
                    _ => return Err(PyException::type_error(format!(
                        "'{}' object does not support item deletion", obj.type_name()))),
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

// ── Group 7: Comparison ──────────────────────────────────────────────
impl VirtualMachine {
    /// Derive a missing comparison from total_ordering root method
    fn derive_total_ordering(
        &mut self, a: &PyObjectRef, b: &PyObjectRef, dunder: &str, root: &str
    ) -> Result<Option<PyObjectRef>, PyException> {
        // Helper: call a's dunder method via the VM
        let call_dunder = |vm: &mut Self, obj: &PyObjectRef, other: &PyObjectRef, method: &str|
            -> Result<Option<bool>, PyException>
        {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let Some(m) = lookup_in_class_mro(&inst.class, method) {
                    let bound = vm.bind_method(obj, m);
                    let r = vm.call_object(bound, vec![other.clone()])?;
                    if !matches!(&r.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(r.is_truthy()));
                    }
                }
            }
            Ok(None)
        };

        // Don't derive if we have the exact root (that should have been found already)
        if dunder == root { return Ok(None); }

        match (root, dunder) {
            ("__lt__", "__le__") => {
                // a <= b  =  a < b or a == b
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    if lt { return Ok(Some(PyObject::bool_val(true))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(eq)));
                    }
                    return Ok(Some(PyObject::bool_val(false)));
                }
            }
            ("__lt__", "__gt__") => {
                // a > b  =  not (a < b) and not (a == b)
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    if lt { return Ok(Some(PyObject::bool_val(false))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(!eq)));
                    }
                    return Ok(Some(PyObject::bool_val(true)));
                }
            }
            ("__lt__", "__ge__") => {
                // a >= b  =  not (a < b)
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    return Ok(Some(PyObject::bool_val(!lt)));
                }
            }
            ("__gt__", "__ge__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    if gt { return Ok(Some(PyObject::bool_val(true))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(eq)));
                    }
                    return Ok(Some(PyObject::bool_val(false)));
                }
            }
            ("__gt__", "__lt__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    if gt { return Ok(Some(PyObject::bool_val(false))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(!eq)));
                    }
                    return Ok(Some(PyObject::bool_val(true)));
                }
            }
            ("__gt__", "__le__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    return Ok(Some(PyObject::bool_val(!gt)));
                }
            }
            _ => {}
        }
        Ok(None)
    }

    pub(crate) fn exec_compare_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let (a, b) = self.vm_pop2();
        self.exec_compare_op(instr.arg, a, b)
    }

    fn exec_compare_op(&mut self, op: u32, a: PyObjectRef, b: PyObjectRef) -> Result<Option<PyObjectRef>, PyException> {
        if let cmp @ 0..=5 = op {
            let (dunder, rdunder) = match cmp {
                0 => ("__lt__", "__gt__"),
                1 => ("__le__", "__ge__"),
                2 => ("__eq__", "__eq__"),
                3 => ("__ne__", "__ne__"),
                4 => ("__gt__", "__lt__"),
                5 => ("__ge__", "__le__"),
                _ => unreachable!()
            };
            // Try a's dunder via MRO walk
            if let PyObjectPayload::Instance(inst) = &a.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, dunder) {
                    let bound = self.bind_method(&a, method);
                    let r = self.call_object(bound, vec![b.clone()])?;
                    if !matches!(&r.payload, PyObjectPayload::NotImplemented) {
                        self.vm_push(r);
                        return Ok(None);
                    }
                }
                // total_ordering fallback: derive missing comparisons from root
                if let Some(root_marker) = lookup_in_class_mro(&inst.class, "__total_ordering_root__") {
                    let root = root_marker.py_to_string();
                    if let Some(result) = self.derive_total_ordering(&a, &b, dunder, &root)? {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
            // Try b's reflected dunder via MRO walk
            if let PyObjectPayload::Instance(inst) = &b.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, rdunder) {
                    let bound = self.bind_method(&b, method);
                    let r = self.call_object(bound, vec![a.clone()])?;
                    if !matches!(&r.payload, PyObjectPayload::NotImplemented) {
                        self.vm_push(r);
                        return Ok(None);
                    }
                }
            }
            // Dataclass auto-equality fallback
            if cmp == 2 || cmp == 3 {
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                    let cls_a = &inst_a.class;
                    if cls_a.get_attr("__dataclass__").is_some() {
                        if let Some(fields) = cls_a.get_attr("__dataclass_fields__") {
                            if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                                let attrs_a = inst_a.attrs.read();
                                let attrs_b = inst_b.attrs.read();
                                let mut eq = true;
                                for ft in field_tuples {
                                    if let PyObjectPayload::Tuple(info) = &ft.payload {
                                        let name = info[0].py_to_string();
                                        let va = attrs_a.get(name.as_str());
                                        let vb = attrs_b.get(name.as_str());
                                        match (va, vb) {
                                            (Some(x), Some(y)) => {
                                                if let Ok(r) = x.compare(y, CompareOp::Eq) {
                                                    if !r.is_truthy() { eq = false; break; }
                                                } else { eq = false; break; }
                                            }
                                            _ => { eq = false; break; }
                                        }
                                    }
                                }
                                let result = if cmp == 2 { eq } else { !eq };
                                self.vm_push(PyObject::bool_val(result));
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            // namedtuple equality: compare underlying _tuple
            if cmp == 2 || cmp == 3 {
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                    if inst_a.class.get_attr("__namedtuple__").is_some() && inst_b.class.get_attr("__namedtuple__").is_some() {
                        let ta = inst_a.attrs.read().get("_tuple").cloned();
                        let tb = inst_b.attrs.read().get("_tuple").cloned();
                        if let (Some(tup_a), Some(tup_b)) = (ta, tb) {
                            let result = tup_a.compare(&tup_b, CompareOp::Eq)?;
                            let val = if cmp == 2 { result.is_truthy() } else { !result.is_truthy() };
                            self.vm_push(PyObject::bool_val(val));
                            return Ok(None);
                        }
                    }
                }
                // namedtuple == plain tuple: compare underlying _tuple with tuple
                if let PyObjectPayload::Instance(inst) = &a.payload {
                    if inst.class.get_attr("__namedtuple__").is_some() {
                        if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                            if matches!(b.payload, PyObjectPayload::Tuple(_)) {
                                let result = tup.compare(&b, CompareOp::Eq)?;
                                let val = if cmp == 2 { result.is_truthy() } else { !result.is_truthy() };
                                self.vm_push(PyObject::bool_val(val));
                                return Ok(None);
                            }
                        }
                    }
                }
                if let PyObjectPayload::Instance(inst) = &b.payload {
                    if inst.class.get_attr("__namedtuple__").is_some() {
                        if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                            if matches!(a.payload, PyObjectPayload::Tuple(_)) {
                                let result = a.compare(&tup, CompareOp::Eq)?;
                                let val = if cmp == 2 { result.is_truthy() } else { !result.is_truthy() };
                                self.vm_push(PyObject::bool_val(val));
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            // IntEnum/enum value-based comparison fallback
            if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                let va = inst_a.attrs.read().get("value").cloned();
                let vb = inst_b.attrs.read().get("value").cloned();
                if let (Some(av), Some(bv)) = (va, vb) {
                    if matches!(av.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                        && matches!(bv.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                    {
                        let cmp_op = match cmp {
                            0 => CompareOp::Lt,
                            1 => CompareOp::Le,
                            2 => CompareOp::Eq,
                            3 => CompareOp::Ne,
                            4 => CompareOp::Gt,
                            5 => CompareOp::Ge,
                            _ => unreachable!()
                        };
                        let result = av.compare(&bv, cmp_op)?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
            // IntEnum vs plain int/float comparison
            {
                let (enum_val, other) = if let PyObjectPayload::Instance(inst) = &a.payload {
                    (inst.attrs.read().get("value").cloned(), Some(&b))
                } else if let PyObjectPayload::Instance(inst) = &b.payload {
                    (inst.attrs.read().get("value").cloned(), Some(&a))
                } else {
                    (None, None)
                };
                if let (Some(ev), Some(ov)) = (enum_val, other) {
                    if matches!(ev.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                        && matches!(ov.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                    {
                        let (left, right) = if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                            (ev, ov.clone())
                        } else {
                            (ov.clone(), ev)
                        };
                        let cmp_op = match cmp {
                            0 => CompareOp::Lt,
                            1 => CompareOp::Le,
                            2 => CompareOp::Eq,
                            3 => CompareOp::Ne,
                            4 => CompareOp::Gt,
                            5 => CompareOp::Ge,
                            _ => unreachable!()
                        };
                        let result = left.compare(&right, cmp_op)?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
        }
        // 'in' / 'not in' with __contains__
        if op == 6 || op == 7 {
            // Handle Class with __contains__ (e.g., Enum: Color.RED in Color)
            if let PyObjectPayload::Class(cd) = &b.payload {
                // Look in own namespace and MRO
                let contains_fn = {
                    let ns = cd.namespace.read();
                    let mut found = ns.get("__contains__").cloned();
                    if found.is_none() {
                        for base in &cd.mro {
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                let bns = bcd.namespace.read();
                                if let Some(f) = bns.get("__contains__") {
                                    found = Some(f.clone());
                                    break;
                                }
                            }
                        }
                    }
                    found
                };
                if let Some(method) = contains_fn {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
            if let PyObjectPayload::Instance(inst) = &b.payload {
                // Check for user-defined __contains__ in the class (including dict subclasses)
                let custom_contains = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    cd.namespace.read().get("__contains__").cloned()
                } else { None };
                if let Some(method) = custom_contains {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Dict subclass: use native contains() directly
                if inst.dict_storage.is_some() {
                    let val = if op == 6 { b.contains(&a)? } else { !b.contains(&a)? };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                if let Some(method) = b.get_attr("__contains__") {
                    let r = self.call_object(method, vec![a])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Fallback: iterate via __iter__ (CPython behavior)
                if let Some(iter_method) = b.get_attr("__iter__") {
                    let iterator = self.call_object(iter_method, vec![])?;
                    let mut found = false;
                    loop {
                        match crate::builtins::iter_advance(&iterator)? {
                            Some((_iter, item)) => {
                                if item.compare(&a, CompareOp::Eq)?.is_truthy() {
                                    found = true;
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    let val = if op == 6 { found } else { !found };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Fallback: iterate via __getitem__ with integer indices (CPython behavior)
                if b.get_attr("__getitem__").is_some() {
                    let mut found = false;
                    let mut idx = 0i64;
                    loop {
                        let getitem = b.get_attr("__getitem__").unwrap();
                        match self.call_object(getitem, vec![PyObject::int(idx)]) {
                            Ok(item) => {
                                if item.compare(&a, CompareOp::Eq)?.is_truthy() {
                                    found = true;
                                    break;
                                }
                                idx += 1;
                            }
                            Err(e) if e.kind == ferrython_core::error::ExceptionKind::IndexError => break,
                            Err(e) => return Err(e),
                        }
                    }
                    let val = if op == 6 { found } else { !found };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
        }
        let result = match op {
            0 => a.compare(&b, CompareOp::Lt)?,
            1 => a.compare(&b, CompareOp::Le)?,
            2 => a.compare(&b, CompareOp::Eq)?,
            3 => a.compare(&b, CompareOp::Ne)?,
            4 => a.compare(&b, CompareOp::Gt)?,
            5 => a.compare(&b, CompareOp::Ge)?,
            6 => PyObject::bool_val(b.contains(&a)?),
            7 => PyObject::bool_val(!b.contains(&a)?),
            8 => PyObject::bool_val(a.is_same(&b)),
            9 => PyObject::bool_val(!a.is_same(&b)),
            10 => {
                let match_one = |a_item: &PyObjectRef, b_item: &PyObjectRef| -> bool {
                    if let PyObjectPayload::Class(cls_a) = &a_item.payload {
                        if let PyObjectPayload::Class(cls_b) = &b_item.payload {
                            if cls_a.name == cls_b.name { return true; }
                            for base in &cls_a.mro {
                                if let PyObjectPayload::Class(bc) = &base.payload {
                                    if bc.name == cls_b.name { return true; }
                                }
                            }
                            for base in &cls_a.bases {
                                if let PyObjectPayload::Class(bc) = &base.payload {
                                    if bc.name == cls_b.name { return true; }
                                }
                            }
                            return false;
                        }
                        if let PyObjectPayload::ExceptionType(kind_b) = &b_item.payload {
                            let kind_a = Self::find_exception_kind(a_item);
                            return exception_kind_matches(&kind_a, kind_b);
                        }
                        return false;
                    }
                    if let PyObjectPayload::ExceptionType(kind_a) = &a_item.payload {
                        return match &b_item.payload {
                            PyObjectPayload::ExceptionType(kind_b) => {
                                exception_kind_matches(kind_a, kind_b)
                            }
                            PyObjectPayload::Class(_cls_b) => {
                                // A built-in exception type (like ValueError) can never be
                                // an instance/subclass of a user-defined exception class
                                // (like AppError), even if they share a common ancestor
                                false
                            }
                            PyObjectPayload::BuiltinType(name) => {
                                if let Some(kind_b) = ExceptionKind::from_name(name) {
                                    exception_kind_matches(kind_a, &kind_b)
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        };
                    }
                    false
                };
                let matched = match &b.payload {
                    PyObjectPayload::Tuple(items) => items.iter().any(|item| match_one(&a, item)),
                    _ => match_one(&a, &b),
                };
                PyObject::bool_val(matched)
            }
            _ => return Err(PyException::runtime_error("invalid compare op")),
        };
        self.vm_push(result);
        Ok(None)
    }
}

// ── Group 8: Jumps + Iterator ────────────────────────────────────────
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
                // Class with __iter__ (e.g. Enum classes): call __iter__(cls)
                if let PyObjectPayload::Class(_) = &obj.payload {
                    // Use get_attr which handles MRO/base class lookup
                    if let Some(iter_method) = obj.get_attr("__iter__") {
                        let result = self.call_object(iter_method, vec![obj.clone()])?;
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
                    self.vm_push(r);
                } else {
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
                            | IteratorData::DropWhile { .. })
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
                    match builtins::iter_advance(&iter)? {
                        Some((new_iter, value)) => {
                            frame.pop();
                            frame.push(new_iter);
                            frame.push(value);
                        }
                        None => {
                            frame.pop();
                            frame.ip = instr.arg as usize;
                        }
                    }
                } else {
                    let frame = self.vm_frame();
                    match builtins::iter_advance(&iter)? {
                        Some((new_iter, value)) => {
                            frame.pop();
                            frame.push(new_iter);
                            frame.push(value);
                        }
                        None => {
                            frame.pop();
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
                    if let PyObjectPayload::Dict(source) = &update_obj.payload {
                        let src = source.read();
                        let mut tgt = target.write();
                        for (k, v) in src.iter() {
                            tgt.insert(k.clone(), v.clone());
                        }
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
                    let new_items = iterable.to_list()?;
                    let mut set = s.write();
                    for item in new_items {
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
                let s_start = if matches!(start.payload, PyObjectPayload::None) { None } else { Some(start) };
                let s_stop = if matches!(stop.payload, PyObjectPayload::None) { None } else { Some(stop) };
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
                    if let PyObjectPayload::Dict(map) = &kw_obj.payload {
                        for (k, v) in map.read().iter() {
                            let name = match k {
                                HashableKey::Str(s) => s.clone(),
                                _ => CompactString::from(format!("{:?}", k)),
                            };
                            kw_vec.push((name, v.clone()));
                        }
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
                    None => return Err(PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", obj.type_name(), name
                    ))),
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
                if flags & 0x04 != 0 { frame.pop(); } // annotations
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
                    PyObjectPayload::Code(c) => Arc::new(*c.clone()),
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
                    annotations: IndexMap::new(),
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
                let frame = self.vm_frame();
                let name = &frame.code.names[instr.arg as usize];
                let module = frame.peek().clone();
                match module.get_attr(name) {
                    Some(v) => frame.push(v),
                    None => return Err(PyException::import_error(format!(
                        "cannot import name '{}' from module", name
                    ))),
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

// ── Group 12: Exception handling + With ──────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_exception_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::SetupFinally => {
                self.vm_frame().push_block(BlockKind::Finally, instr.arg as usize);
            }
            Opcode::SetupExcept => {
                self.vm_frame().push_block(BlockKind::Except, instr.arg as usize);
            }
            Opcode::PopBlock => { self.vm_frame().pop_block(); }
            Opcode::PopExcept => {
                self.vm_frame().pop_block();
                self.active_exception = None;
                ferrython_stdlib::clear_exc_info();
            }
            Opcode::EndFinally => {
                return self.exec_end_finally();
            }
            Opcode::BeginFinally => {
                self.vm_frame().push(PyObject::none());
            }
            Opcode::RaiseVarargs => {
                return self.exec_raise_varargs(instr.arg);
            }
            Opcode::SetupWith => {
                return self.exec_setup_with(instr.arg);
            }

            Opcode::SetupAsyncWith => {
                // At this point, __aenter__() has already been called and awaited.
                // TOS = result of __aenter__ (the value for `as` clause).
                // Below TOS = the async context manager.
                // We need to get __aexit__ and push it for cleanup, then set up With block.
                let enter_result = self.vm_pop();
                let ctx_mgr = self.vm_pop();
                let exit_raw = ctx_mgr.get_attr("__aexit__").ok_or_else(||
                    PyException::attribute_error("__aexit__"))?;
                let exit_method = if matches!(&exit_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                    exit_raw
                } else {
                    Arc::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: ctx_mgr.clone(),
                            method: exit_raw,
                        }
                    })
                };
                self.vm_push(exit_method);
                let frame = self.vm_frame();
                frame.push_block(BlockKind::With, instr.arg as usize);
                frame.push(enter_result);
            }

            Opcode::WithCleanupStart => {
                let tos = self.vm_frame().peek().clone();
                // Extract __closing_thing__ from context manager (for contextlib.closing)
                // We peek at exit_fn (2nd from top) to get the receiver before consumption
                let closing_thing = {
                    let stack = &self.vm_frame().stack;
                    if stack.len() >= 2 {
                        let exit_fn_ref = &stack[stack.len() - 2];
                        if let PyObjectPayload::BoundMethod { receiver, .. } = &exit_fn_ref.payload {
                            receiver.get_attr("__closing_thing__")
                        } else { None }
                    } else { None }
                };
                if matches!(tos.payload, PyObjectPayload::None) {
                    // Normal exit (no exception)
                    self.vm_pop(); // pop None
                    let exit_fn = self.vm_pop();
                    if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {}
                            Err(e) if e.kind == ExceptionKind::StopIteration => {}
                            Err(e) => return Err(e),
                        }
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(PyObject::none());
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            PyObject::none(), PyObject::none(), PyObject::none()
                        ])?;
                        // Call close() on closing thing if present
                        if let Some(thing) = &closing_thing {
                            self.call_close_on(thing)?;
                        }
                        // If __aexit__ returns a coroutine, drive it to completion
                        let result = self.maybe_await_result(result)?;
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(result);
                    }
                } else if matches!(tos.payload, PyObjectPayload::ExceptionType(_))
                       || matches!(tos.payload, PyObjectPayload::Class(_)) {
                    // Exception exit: stack has [exit_fn, tb, value, type]
                    let exc_type = self.vm_pop();
                    let exc_val = if !self.vm_frame().stack.is_empty() { self.vm_pop() } else { PyObject::none() };
                    let exc_tb = if !self.vm_frame().stack.is_empty() { self.vm_pop() } else { PyObject::none() };
                    let exit_fn = self.vm_pop();
                    if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                        // Throw exception into generator so its except clauses can catch it
                        let exc_kind = match &exc_type.payload {
                            PyObjectPayload::ExceptionType(k) => k.clone(),
                            PyObjectPayload::Class(_) => Self::find_exception_kind(&exc_type),
                            _ => ExceptionKind::RuntimeError,
                        };
                        let exc_msg = match &exc_val.payload {
                            PyObjectPayload::ExceptionInstance { message, .. } => message.to_string(),
                            _ => exc_val.py_to_string(),
                        };
                        let gen_arc_clone = gen_arc.clone();
                        match self.gen_throw(&gen_arc_clone, exc_kind, exc_msg) {
                            Ok(_) | Err(PyException { kind: ExceptionKind::StopIteration, .. }) => {
                                // Generator handled exception (suppressed)
                                let f = self.vm_frame();
                                f.push(PyObject::none());
                                f.push(PyObject::none());
                                f.push(PyObject::none());
                                f.push(PyObject::bool_val(true));
                            }
                            Err(_e) => {
                                // Generator re-raised or raised a different exception
                                let f = self.vm_frame();
                                f.push(exc_tb);
                                f.push(exc_val);
                                f.push(exc_type.clone());
                                f.push(PyObject::none());
                            }
                        }
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            exc_type.clone(), exc_val.clone(), exc_tb.clone()
                        ])?;
                        // Call close() on closing thing if present
                        if let Some(thing) = &closing_thing {
                            let _ = self.call_close_on(thing);
                        }
                        // If __aexit__ returns a coroutine, drive it to completion
                        let result = self.maybe_await_result(result)?;
                        let f = self.vm_frame();
                        // Preserve exception info for EndFinally re-raise
                        f.push(exc_tb);
                        f.push(exc_val);
                        f.push(exc_type);
                        f.push(result);
                    }
                } else {
                    self.vm_pop();
                    let exit_fn = self.vm_pop();
                    if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {}
                            Err(e) if e.kind == ExceptionKind::StopIteration => {}
                            Err(e) => return Err(e),
                        }
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(PyObject::none());
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            PyObject::none(), PyObject::none(), PyObject::none()
                        ])?;
                        // Call close() on closing thing if present
                        if let Some(thing) = &closing_thing {
                            let _ = self.call_close_on(thing);
                        }
                        let result = self.maybe_await_result(result)?;
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(result);
                    }
                }
            }
            Opcode::WithCleanupFinish => {
                let frame = self.vm_frame();
                let exit_result = frame.pop();
                let exc_or_none = frame.pop();
                if !matches!(exc_or_none.payload, PyObjectPayload::None) && exit_result.is_truthy() {
                    // Exception was suppressed: clean up exception info (value, tb)
                    frame.pop(); // value
                    frame.pop(); // tb
                    frame.push(PyObject::none());
                } else if !matches!(exc_or_none.payload, PyObjectPayload::None) {
                    // Exception NOT suppressed: push type back, leave (tb, value) for EndFinally
                    frame.push(exc_or_none);
                } else {
                    // No exception
                    frame.push(exc_or_none);
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }

    fn exec_end_finally(&mut self) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        if let Some(ret_val) = frame.pending_return.take() {
            let mut has_finally = false;
            while let Some(block) = frame.block_stack.last() {
                if block.kind == BlockKind::Finally {
                    let handler = block.handler;
                    frame.block_stack.pop();
                    frame.pending_return = Some(ret_val.clone());
                    frame.push(PyObject::none());
                    frame.ip = handler;
                    has_finally = true;
                    break;
                } else {
                    frame.block_stack.pop();
                }
            }
            if !has_finally {
                return Ok(Some(ret_val));
            }
        } else {
            if !frame.stack.is_empty() {
                let tos = frame.peek();
                match &tos.payload {
                    PyObjectPayload::ExceptionType(kind) => {
                        let kind = kind.clone();
                        frame.pop();
                        let value = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        if !frame.stack.is_empty() { frame.pop(); }
                        let msg = match &value.payload {
                            PyObjectPayload::ExceptionInstance { message, .. } => message.to_string(),
                            _ => value.py_to_string(),
                        };
                        return Err(PyException::new(kind, msg));
                    }
                    PyObjectPayload::Class(_) => {
                        // User-defined exception class on stack — re-raise
                        let cls = frame.pop();
                        let kind = Self::find_exception_kind(&cls);
                        let value = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        if !frame.stack.is_empty() { frame.pop(); }
                        let msg = match &value.payload {
                            PyObjectPayload::ExceptionInstance { message, .. } => message.to_string(),
                            PyObjectPayload::Instance(_) => {
                                if let Some(args) = value.get_attr("args") {
                                    args.py_to_string()
                                } else {
                                    value.py_to_string()
                                }
                            }
                            _ => value.py_to_string(),
                        };
                        return Err(PyException::with_original(kind, msg, value));
                    }
                    PyObjectPayload::None => { frame.pop(); }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn exec_raise_varargs(&mut self, argc: u32) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        let raise_exc = |exc: &PyObjectRef| -> PyException {
            match &exc.payload {
                PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                    PyException::with_original(kind.clone(), message.to_string(), exc.clone())
                }
                PyObjectPayload::ExceptionType(kind) => {
                    PyException::new(kind.clone(), "")
                }
                PyObjectPayload::Instance(inst) => {
                    let kind = Self::find_exception_kind(&inst.class);
                    // Derive message from args (CPython: str(exc) uses args)
                    let msg = if let Some(a) = exc.get_attr("args") {
                        if let PyObjectPayload::Tuple(items) = &a.payload {
                            if items.len() == 1 {
                                items[0].py_to_string()
                            } else if items.is_empty() {
                                String::new()
                            } else {
                                a.repr()
                            }
                        } else {
                            exc.py_to_string()
                        }
                    } else {
                        exc.py_to_string()
                    };
                    PyException::with_original(kind, msg, exc.clone())
                }
                PyObjectPayload::Class(_) => {
                    let kind = Self::find_exception_kind(exc);
                    PyException::new(kind, "")
                }
                _ => PyException::runtime_error(exc.py_to_string()),
            }
        };
        match argc {
            0 => {
                // Bare raise: re-raise the currently active exception
                if let Some(exc) = self.active_exception.clone() {
                    return Err(exc);
                }
                return Err(PyException::runtime_error("No active exception to re-raise"));
            }
            1 => {
                let exc = frame.pop();
                let mut py_exc = raise_exc(&exc);
                // Implicit chaining: set __context__ to active exception
                if let Some(active) = &self.active_exception {
                    py_exc.context = Some(Box::new(active.clone()));
                }
                return Err(py_exc);
            }
            2 => {
                let cause = frame.pop();
                let exc = frame.pop();
                let mut py_exc = raise_exc(&exc);
                // `raise X from None` suppresses the cause
                if matches!(cause.payload, PyObjectPayload::None) {
                    // raise X from None: suppress context display
                    if let Some(ref original) = py_exc.original {
                        if let PyObjectPayload::ExceptionInstance { attrs, .. } = &original.payload {
                            let mut w = attrs.write();
                            w.insert(CompactString::from("__cause__"), PyObject::none());
                            w.insert(CompactString::from("__suppress_context__"), PyObject::bool_val(true));
                        }
                    }
                } else {
                    let cause_exc = raise_exc(&cause);
                    // Store __cause__ on the exception instance's attrs
                    if let Some(ref original) = py_exc.original {
                        if let PyObjectPayload::ExceptionInstance { attrs, .. } = &original.payload {
                            let mut w = attrs.write();
                            w.insert(CompactString::from("__cause__"), cause.clone());
                            w.insert(CompactString::from("__suppress_context__"), PyObject::bool_val(true));
                        }
                    }
                    py_exc.cause = Some(Box::new(cause_exc));
                }
                // Implicit chaining: set __context__ to active exception
                if let Some(active) = &self.active_exception {
                    py_exc.context = Some(Box::new(active.clone()));
                    if let Some(ref original) = py_exc.original {
                        if let PyObjectPayload::ExceptionInstance { attrs, .. } = &original.payload {
                            // Store __context__ as the active exception's original object
                            if let Some(ref ctx_orig) = active.original {
                                attrs.write().insert(CompactString::from("__context__"), ctx_orig.clone());
                            }
                        }
                    }
                }
                return Err(py_exc);
            }
            _ => return Err(PyException::runtime_error("bad RAISE_VARARGS arg")),
        }
    }

    fn exec_setup_with(&mut self, arg: u32) -> Result<Option<PyObjectRef>, PyException> {
        let ctx_mgr = self.vm_pop();
        if let PyObjectPayload::Generator(gen_arc) = &ctx_mgr.payload {
            let enter_result = match self.resume_generator(gen_arc, PyObject::none()) {
                Ok(val) => val,
                Err(e) if e.kind == ExceptionKind::StopIteration => PyObject::none(),
                Err(e) => return Err(e),
            };
            let frame = self.vm_frame();
            frame.push(ctx_mgr.clone());
            frame.push_block(BlockKind::With, arg as usize);
            frame.push(enter_result);
        } else {
            let exit_raw = ctx_mgr.get_attr("__exit__").ok_or_else(||
                PyException::attribute_error("__exit__"))?;
            // Bind exit to ctx_mgr so WithCleanupStart passes self correctly
            let exit_method = if matches!(&exit_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                exit_raw
            } else {
                Arc::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: ctx_mgr.clone(),
                        method: exit_raw,
                    }
                })
            };
            self.vm_push(exit_method);
            let enter_raw = ctx_mgr.get_attr("__enter__").ok_or_else(||
                PyException::attribute_error("__enter__"))?;
            let (enter_method, enter_args) = if matches!(&enter_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                (enter_raw, vec![])
            } else {
                let bound = Arc::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: ctx_mgr.clone(),
                        method: enter_raw,
                    }
                });
                (bound, vec![])
            };
            let enter_result = self.call_object(enter_method, enter_args)?;
            let frame = self.vm_frame();
            frame.push_block(BlockKind::With, arg as usize);
            frame.push(enter_result);
        }
        Ok(None)
    }
}

// ── Group 13: Misc ops ───────────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_misc_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::PrintExpr => {
                let frame = self.vm_frame();
                let value = frame.pop();
                if !matches!(value.payload, PyObjectPayload::None) {
                    println!("{}", value.repr());
                }
            }
            Opcode::LoadBuildClass => {
                self.vm_frame().push(PyObject::builtin_function(
                    CompactString::from("__build_class__")));
            }
            Opcode::SetupAnnotations => {
                let frame = self.vm_frame();
                if !frame.local_names.contains_key("__annotations__") {
                    frame.store_name(
                        CompactString::from("__annotations__"),
                        PyObject::dict(IndexMap::new()),
                    );
                }
            }
            Opcode::FormatValue => {
                let frame = self.vm_frame();
                let fmt_spec = if instr.arg & 0x04 != 0 {
                    let spec_obj = frame.pop();
                    spec_obj.as_str().unwrap_or("").to_string()
                } else {
                    String::new()
                };
                let value = frame.pop();
                let conversion = (instr.arg & 0x03) as u8;
                let base_str = match conversion {
                    1 => {
                        // !s conversion — use VM-aware str for instances
                        if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                            self.vm_str(&value)?
                        } else {
                            value.py_to_string()
                        }
                    }
                    2 => {
                        // !r conversion — use VM-aware repr for instances
                        if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                            self.vm_repr(&value)?
                        } else {
                            value.repr()
                        }
                    }
                    3 => {
                        // !a conversion — ascii repr (same as repr for ASCII strings)
                        if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                            self.vm_repr(&value)?
                        } else {
                            value.repr()
                        }
                    }
                    _ => {
                        if !fmt_spec.is_empty() {
                            if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                                if let Some(format_method) = value.get_attr("__format__") {
                                    let spec = PyObject::str_val(CompactString::from(&fmt_spec));
                                    let r = self.call_object(format_method, vec![spec])?;
                                    self.vm_push(PyObject::str_val(CompactString::from(r.py_to_string())));
                                    return Ok(None);
                                }
                            }
                            match value.format_value(&fmt_spec) {
                                Ok(s) => s,
                                Err(_) => value.py_to_string(),
                            }
                        } else {
                            if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                                if let Some(format_method) = value.get_attr("__format__") {
                                    let spec = PyObject::str_val(CompactString::from(""));
                                    let r = self.call_object(format_method, vec![spec])?;
                                    self.vm_push(PyObject::str_val(CompactString::from(r.py_to_string())));
                                    return Ok(None);
                                }
                                if let Some(str_method) = value.get_attr("__str__") {
                                    let r = self.call_object(str_method, vec![])?;
                                    let s = r.py_to_string();
                                    self.vm_push(PyObject::str_val(CompactString::from(s)));
                                    return Ok(None);
                                }
                            }
                            // Use VM-aware str for containers (list/tuple/dict)
                            // so items with __repr__ get proper representation
                            self.vm_str(&value)?
                        }
                    }
                };
                let formatted = if !fmt_spec.is_empty() && conversion != 0 {
                    use ferrython_core::object::apply_string_format_spec;
                    apply_string_format_spec(&base_str, &fmt_spec)
                } else {
                    base_str
                };
                self.vm_push(PyObject::str_val(CompactString::from(formatted)));
            }
            Opcode::ExtendedArg => {}
            Opcode::YieldValue => {
                let frame = self.vm_frame();
                let value = frame.pop();
                frame.yielded = true;
                return Ok(Some(value));
            }
            Opcode::YieldFrom => {
                let send_val = self.vm_pop();
                let sub_iter = self.vm_frame().peek().clone();

                // Handle Generator, Coroutine, and AsyncGenerator using same resume mechanism
                let gen_arc_opt = match &sub_iter.payload {
                    PyObjectPayload::Generator(ref g) => Some(g.clone()),
                    PyObjectPayload::Coroutine(ref g) => Some(g.clone()),
                    PyObjectPayload::AsyncGenerator(ref g) => Some(g.clone()),
                    _ => None,
                };

                if let Some(gen_arc) = gen_arc_opt {
                    match self.resume_generator(&gen_arc, send_val) {
                        Ok(yielded) => {
                            let frame = self.vm_frame();
                            frame.yielded = true;
                            frame.ip -= 1;
                            return Ok(Some(yielded));
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            let frame = self.vm_frame();
                            frame.pop();
                            // yield from captures StopIteration.value as the result
                            let return_val = e.value.unwrap_or_else(|| PyObject::none());
                            frame.push(return_val);
                        }
                        Err(e) => return Err(e),
                    }
                } else if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &sub_iter.payload {
                    // Drive the async generator awaitable — this is what happens when
                    // `await ag.__anext__()` is compiled as GetAwaitable + YieldFrom.
                    match self.drive_async_gen_awaitable(gen, action, send_val) {
                        Ok(yielded) => {
                            // Intermediate yield — propagate up to the driving coroutine
                            let frame = self.vm_frame();
                            frame.yielded = true;
                            frame.ip -= 1;
                            return Ok(Some(yielded));
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            let frame = self.vm_frame();
                            frame.pop();
                            let return_val = e.value.unwrap_or_else(|| PyObject::none());
                            frame.push(return_val);
                        }
                        Err(e) => return Err(e),
                    }
                } else if let PyObjectPayload::BuiltinAwaitable(inner_val) = &sub_iter.payload {
                    // BuiltinAwaitable: immediately resolve with the stored value.
                    // If the value is a list of coroutines (from asyncio.gather),
                    // drive each one and collect results.
                    let result = if let PyObjectPayload::List(items) = &inner_val.payload {
                        let items = items.read().clone();
                        let has_coro = items.iter().any(|item| matches!(&item.payload, PyObjectPayload::Coroutine(_)));
                        if has_coro {
                            // asyncio.gather pattern: drive each coroutine
                            let mut results = Vec::with_capacity(items.len());
                            for item in &items {
                                let r = self.maybe_await_result(item.clone())?;
                                results.push(r);
                            }
                            PyObject::list(results)
                        } else {
                            inner_val.clone()
                        }
                    } else {
                        inner_val.clone()
                    };
                    let frame = self.vm_frame();
                    frame.pop();
                    frame.push(result);
                } else if matches!(&sub_iter.payload, PyObjectPayload::Instance(_)) {
                    if let Some(next_method) = sub_iter.get_attr("__next__") {
                        match self.call_object(next_method, vec![]) {
                            Ok(val) => {
                                let frame = self.vm_frame();
                                frame.yielded = true;
                                frame.ip -= 1;
                                return Ok(Some(val));
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                let frame = self.vm_frame();
                                frame.pop();
                                frame.push(PyObject::none());
                            }
                            Err(e) => return Err(e),
                        }
                    } else {
                        let frame = self.vm_frame();
                        frame.pop();
                        frame.push(PyObject::none());
                    }
                } else {
                    let frame = self.vm_frame();
                    match builtins::iter_advance(&sub_iter)? {
                        Some((new_iter, value)) => {
                            frame.pop();
                            frame.push(new_iter);
                            frame.yielded = true;
                            frame.ip -= 1;
                            return Ok(Some(value));
                        }
                        None => {
                            frame.pop();
                            frame.push(PyObject::none());
                        }
                    }
                }
            }
            // ── Async opcodes ──

            Opcode::GetAwaitable => {
                // TOS is a coroutine or object with __await__. Push the awaitable iterator.
                let obj = self.vm_pop();
                match &obj.payload {
                    // Coroutine is already awaitable — push it directly
                    PyObjectPayload::Coroutine(_) => {
                        self.vm_push(obj);
                    }
                    // AsyncGenAwaitable (from __anext__, asend, athrow, aclose) is awaitable
                    PyObjectPayload::AsyncGenAwaitable { .. } => {
                        self.vm_push(obj);
                    }
                    // Generator marked as iterable_coroutine (types.coroutine)
                    PyObjectPayload::Generator(_) => {
                        self.vm_push(obj);
                    }
                    // BuiltinAwaitable — native awaitable from asyncio.sleep(), gather(), etc.
                    PyObjectPayload::BuiltinAwaitable(_) => {
                        self.vm_push(obj);
                    }
                    _ => {
                        // Try __await__() protocol — returns an iterator
                        if let Some(await_method) = obj.get_attr("__await__") {
                            let iter = self.call_object(await_method, vec![])?;
                            self.vm_push(iter);
                        } else {
                            return Err(PyException::type_error(format!(
                                "object {} can't be used in 'await' expression",
                                obj.type_name()
                            )));
                        }
                    }
                }
            }

            Opcode::GetAiter => {
                // TOS = async iterable. Call __aiter__() and push result.
                let obj = self.vm_pop();
                if let Some(aiter_method) = obj.get_attr("__aiter__") {
                    let aiter = self.call_object(aiter_method, vec![])?;
                    self.vm_push(aiter);
                } else {
                    return Err(PyException::type_error(format!(
                        "'{}' object is not an async iterable",
                        obj.type_name()
                    )));
                }
            }

            Opcode::GetAnext => {
                // TOS = async iterator. Call __anext__() which returns an awaitable.
                let aiter = self.vm_frame().peek().clone();
                if let Some(anext_method) = aiter.get_attr("__anext__") {
                    let awaitable = self.call_object(anext_method, vec![])?;
                    self.vm_push(awaitable);
                } else {
                    return Err(PyException::type_error(format!(
                        "'{}' object is not an async iterator",
                        aiter.type_name()
                    )));
                }
            }

            Opcode::BeforeAsyncWith => {
                // TOS = async context manager. Call __aenter__() → push awaitable result.
                // Keep ctx_mgr on stack (peek) — SetupAsyncWith will pop it later.
                let ctx_mgr = self.vm_frame().peek().clone();
                let aenter_raw = ctx_mgr.get_attr("__aenter__").ok_or_else(||
                    PyException::type_error(format!(
                        "'{}' object does not support the async context manager protocol",
                        ctx_mgr.type_name()
                    )))?;
                let (aenter_method, aenter_args) = if matches!(&aenter_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                    (aenter_raw, vec![])
                } else {
                    let bound = Arc::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: ctx_mgr.clone(),
                            method: aenter_raw,
                        }
                    });
                    (bound, vec![])
                };
                let result = self.call_object(aenter_method, aenter_args)?;
                self.vm_push(result);
            }

            Opcode::EndAsyncFor => {
                // End of async for — check if exception is StopAsyncIteration.
                // Stack: [... aiter, exception] → [...]
                let exc = self.vm_pop();
                let _aiter = self.vm_pop();
                // If it's StopAsyncIteration, the loop ends normally.
                // Otherwise, re-raise the exception.
                let is_stop_async = match &exc.payload {
                    PyObjectPayload::ExceptionType(k) => *k == ExceptionKind::StopAsyncIteration,
                    PyObjectPayload::ExceptionInstance { kind, .. } => *kind == ExceptionKind::StopAsyncIteration,
                    _ => false,
                };
                if !is_stop_async {
                    // Check if the active exception is StopAsyncIteration
                    if let Some(ref active) = self.active_exception {
                        if active.kind != ExceptionKind::StopAsyncIteration {
                            let e = active.clone();
                            self.active_exception = None;
                            return Err(e);
                        }
                    }
                }
                self.active_exception = None;
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
