//! Opcode group handlers for the VM.
//!
//! This module splits the monolithic `execute_one` match into logically
//! grouped methods, each handling a family of related opcodes.

use crate::builtins;
use crate::frame::{BlockKind, Frame, ScopeKind};
use crate::vm::{constant_to_object, exception_kind_matches, VirtualMachine};
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyFunction};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

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
                let constant = &frame.code.constants[idx];
                let obj = constant_to_object(constant);
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
                let name = &frame.code.names[instr.arg as usize];
                frame.local_names.shift_remove(name.as_str());
                frame.globals.write().shift_remove(name.as_str());
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
        Ok(None)
    }
}

// ── Group 3: Attribute operations ────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_attr_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::LoadAttr => {
                let frame = self.vm_frame();
                let name = frame.code.names[instr.arg as usize].clone();
                let obj = frame.pop();
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
                match obj.get_attr(&name) {
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
                        } else if has_descriptor_get(&v) {
                            // Custom descriptor protocol: call __get__(self, instance, owner)
                            let get_method = v.get_attr("__get__").unwrap();
                            let owner = if let PyObjectPayload::Instance(inst) = &obj.payload {
                                inst.class.clone()
                            } else {
                                PyObject::none()
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
                            let result = self.call_object(get_method_bound, vec![obj, owner])?;
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
                            if let Some(type_method) = builtins::resolve_type_class_method(tn, &name) {
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
            }
            Opcode::StoreAttr => {
                let frame = self.vm_frame();
                let name = frame.code.names[instr.arg as usize].clone();
                let obj = frame.pop();
                let value = frame.pop();
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(desc) = lookup_in_class_mro(&inst.class, &name) {
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
                        if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                            let method = Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: sa,
                                }
                            });
                            let name_arg = PyObject::str_val(name);
                            self.call_object(method, vec![name_arg, value])?;
                            return Ok(None);
                        }
                    }
                }
                match &obj.payload {
                    PyObjectPayload::Instance(inst) => {
                        inst.attrs.write().insert(name, value);
                    }
                    PyObjectPayload::Class(cd) => {
                        cd.namespace.write().insert(name, value);
                    }
                    _ => {
                        return Err(PyException::attribute_error(format!(
                            "'{}' object does not support attribute assignment", obj.type_name()
                        )));
                    }
                }
            }
            Opcode::DeleteAttr => {
                let frame = self.vm_frame();
                let name = frame.code.names[instr.arg as usize].clone();
                let obj = frame.pop();
                match &obj.payload {
                    PyObjectPayload::Instance(inst) => {
                        if let Some(delattr_method) = lookup_in_class_mro(&inst.class, "__delattr__") {
                            if matches!(&delattr_method.payload, PyObjectPayload::Function(_)) {
                                let method = Arc::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod { receiver: obj.clone(), method: delattr_method }
                                });
                                let name_arg = PyObject::str_val(name);
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
                    }
                    _ => return Err(PyException::attribute_error(format!(
                        "'{}' object does not support attribute deletion", obj.type_name()))),
                }
            }
            _ => unreachable!(),
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
    fn try_binary_dunder(
        &mut self, a: &PyObjectRef, b: &PyObjectRef,
        dunder: &str, rdunder: Option<&str>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if matches!(&a.payload, PyObjectPayload::Instance(_)) {
            if let Some(m) = a.get_attr(dunder) {
                return Ok(Some(self.call_object(m, vec![b.clone()])?));
            }
        }
        if let Some(rd) = rdunder {
            if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                if let Some(m) = b.get_attr(rd) {
                    return Ok(Some(self.call_object(m, vec![a.clone()])?));
                }
            }
        }
        Ok(None)
    }

    fn try_inplace_dunder(
        &mut self, a: &PyObjectRef, b: &PyObjectRef,
        idunder: &str, dunder: &str,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if matches!(&a.payload, PyObjectPayload::Instance(_)) {
            if let Some(m) = a.get_attr(idunder).or_else(|| a.get_attr(dunder)) {
                return Ok(Some(self.call_object(m, vec![b.clone()])?));
            }
        }
        Ok(None)
    }

    pub(crate) fn exec_binary_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let (a, b) = self.vm_pop2();
        let result = match instr.op {
            Opcode::BinaryAdd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__add__", Some("__radd__"))? { r }
                else { a.add(&b)? }
            }
            Opcode::InplaceAdd => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__iadd__", "__add__")? { r }
                else { a.add(&b)? }
            }
            Opcode::BinarySubtract => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? { r }
                else { a.sub(&b)? }
            }
            Opcode::InplaceSubtract => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__isub__", "__sub__")? { r }
                else { a.sub(&b)? }
            }
            Opcode::BinaryMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mul__", Some("__rmul__"))? { r }
                else { a.mul(&b)? }
            }
            Opcode::InplaceMultiply => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imul__", "__mul__")? { r }
                else { a.mul(&b)? }
            }
            Opcode::BinaryTrueDivide => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__truediv__", Some("__rtruediv__"))? { r }
                else { a.true_div(&b)? }
            }
            Opcode::InplaceTrueDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__itruediv__", "__truediv__")? { r }
                else { a.true_div(&b)? }
            }
            Opcode::BinaryFloorDivide => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__floordiv__", Some("__rfloordiv__"))? { r }
                else { a.floor_div(&b)? }
            }
            Opcode::InplaceFloorDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ifloordiv__", "__floordiv__")? { r }
                else { a.floor_div(&b)? }
            }
            Opcode::BinaryModulo | Opcode::InplaceModulo => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mod__", Some("__rmod__"))? { r }
                else { a.modulo(&b)? }
            }
            Opcode::BinaryPower | Opcode::InplacePower => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__pow__", Some("__rpow__"))? { r }
                else { a.power(&b)? }
            }
            Opcode::BinaryLshift | Opcode::InplaceLshift => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__lshift__", None)? { r }
                else { a.lshift(&b)? }
            }
            Opcode::BinaryRshift | Opcode::InplaceRshift => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__rshift__", None)? { r }
                else { a.rshift(&b)? }
            }
            Opcode::BinaryAnd | Opcode::InplaceAnd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__and__", None)? { r }
                else { a.bit_and(&b)? }
            }
            Opcode::BinaryOr | Opcode::InplaceOr => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__or__", None)? { r }
                else { a.bit_or(&b)? }
            }
            Opcode::BinaryXor | Opcode::InplaceXor => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__xor__", None)? { r }
                else { a.bit_xor(&b)? }
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
                let key = self.vm_pop();
                let obj = self.vm_pop();
                // __class_getitem__: MyClass[int] → MyClass.__class_getitem__(cls, int)
                if matches!(&obj.payload, PyObjectPayload::Class(_)) {
                    if let Some(cgi) = obj.get_attr("__class_getitem__") {
                        let result = self.call_object(cgi, vec![obj.clone(), key])?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
                if let Some(r) = self.try_call_dunder(&obj, "__getitem__", vec![key.clone()])? {
                    self.vm_push(r);
                    return Ok(None);
                }
                if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
                    if let Some(tup) = obj.get_attr("_tuple") {
                        self.vm_push(tup.get_item(&key)?);
                        return Ok(None);
                    }
                }
                if let PyObjectPayload::Dict(map) = &obj.payload {
                    let hk = key.to_hashable_key()?;
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
                        if let PyObjectPayload::Slice { start, stop, step: _ } = &key.payload {
                            let new_items = value.to_list()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let s_val = start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0);
                            let e_val = stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len);
                            let s = (if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) }) as usize;
                            let e = (if e_val < 0 { (len + e_val).max(0) } else { e_val.min(len) }) as usize;
                            let e = e.max(s);
                            w.splice(s..e, new_items);
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
                        let hk = key.to_hashable_key()?;
                        map.write().insert(hk, value);
                    }
                    PyObjectPayload::Instance(_) => {
                        if let Some(m) = obj.get_attr("__setitem__") {
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
                        let idx = key.to_int()?;
                        let mut w = items.write();
                        let len = w.len() as i64;
                        let actual = if idx < 0 { len + idx } else { idx };
                        if actual < 0 || actual >= len {
                            return Err(PyException::index_error("list assignment index out of range"));
                        }
                        w.remove(actual as usize);
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = key.to_hashable_key()?;
                        if map.write().swap_remove(&hk).is_none() {
                            return Err(PyException::key_error(key.repr()));
                        }
                    }
                    PyObjectPayload::Instance(_) => {
                        if let Some(method) = obj.get_attr("__delitem__") {
                            self.call_object(method, vec![key])?;
                            return Ok(None);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object does not support item deletion", obj.type_name())));
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
    pub(crate) fn exec_compare_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let (a, b) = self.vm_pop2();
        if let op @ 0..=5 = instr.arg {
            let dunder = match op {
                0 => "__lt__", 1 => "__le__", 2 => "__eq__",
                3 => "__ne__", 4 => "__gt__", 5 => "__ge__",
                _ => unreachable!()
            };
            if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                if let Some(method) = a.get_attr(dunder) {
                    let r = self.call_object(method, vec![b])?;
                    self.vm_push(r);
                    return Ok(None);
                }
                // Dataclass auto-equality
                if op == 2 || op == 3 {
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
                                    let result = if op == 2 { eq } else { !eq };
                                    self.vm_push(PyObject::bool_val(result));
                                    return Ok(None);
                                }
                            }
                        }
                    }
                }
            }
        }
        // 'in' / 'not in' with __contains__
        if instr.arg == 6 || instr.arg == 7 {
            if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                if let Some(method) = b.get_attr("__contains__") {
                    let r = self.call_object(method, vec![a])?;
                    let val = if instr.arg == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
        }
        let result = match instr.arg {
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
                            PyObjectPayload::Class(_) => {
                                let kind_b = Self::find_exception_kind(b_item);
                                exception_kind_matches(kind_a, &kind_b)
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
                if let Some(r) = self.try_call_dunder(&obj, "__iter__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(obj.get_iter()?);
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
                } else if matches!(&iter.payload, PyObjectPayload::Instance(_)) {
                    if let Some(next_method) = iter.get_attr("__next__") {
                        match self.call_object(next_method, vec![]) {
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
                let mut set = IndexMap::new();
                for item in stack_items {
                    if let Ok(key) = item.to_hashable_key() {
                        set.insert(key, item);
                    }
                }
                frame.push(PyObject::set(set));
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
                let mut map = IndexMap::new();
                for (key, value) in entries {
                    let hkey = key.to_hashable_key()?;
                    map.insert(hkey, value);
                }
                frame.push(PyObject::dict(map));
            }
            Opcode::BuildConstKeyMap => {
                let keys_tuple = frame.pop();
                let keys = keys_tuple.to_list()?;
                let count = instr.arg as usize;
                let mut values = Vec::new();
                for _ in 0..count { values.push(frame.pop()); }
                values.reverse();
                let mut map = IndexMap::new();
                for (key, value) in keys.into_iter().zip(values) {
                    let hkey = key.to_hashable_key()?;
                    map.insert(hkey, value);
                }
                frame.push(PyObject::dict(map));
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
                if let PyObjectPayload::Set(s) = &set_obj.payload {
                    if let Ok(key) = item.to_hashable_key() {
                        s.write().insert(key, item);
                    }
                }
            }
            Opcode::MapAdd => {
                let value = frame.pop();
                let key = frame.pop();
                let idx = instr.arg as usize;
                let stack_pos = frame.stack.len() - idx;
                let dict_obj = &frame.stack[stack_pos];
                if let PyObjectPayload::Dict(m) = &dict_obj.payload {
                    if let Ok(hk) = key.to_hashable_key() {
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
                    let new_items = iterable.to_list()?;
                    items.write().extend(new_items);
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
                        if let Ok(key) = item.to_hashable_key() {
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
                let seq = frame.pop();
                let items = seq.to_list()?;
                let count = instr.arg as usize;
                if items.len() != count {
                    return Err(PyException::value_error(format!(
                        "not enough values to unpack (expected {}, got {})",
                        count, items.len()
                    )));
                }
                for item in items.into_iter().rev() {
                    frame.push(item);
                }
            }
            Opcode::UnpackEx => {
                let seq = frame.pop();
                let items = seq.to_list()?;
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
                let result = self.call_object(func, args)?;
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
                let result = self.call_object_kw(func, pos_args, kwargs)?;
                self.vm_push(result);
            }
            Opcode::CallMethod => {
                let frame = self.vm_frame();
                let arg_count = instr.arg as usize;
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count { args.push(frame.pop()); }
                args.reverse();
                let method = frame.pop();
                let result = self.call_object(method, args)?;
                self.vm_push(result);
            }
            Opcode::CallFunctionEx => {
                let frame = self.vm_frame();
                let has_kwargs = (instr.arg & 1) != 0;
                let kwargs_obj = if has_kwargs { Some(frame.pop()) } else { None };
                let args_obj = frame.pop();
                let func = frame.pop();
                let pos_args = args_obj.to_list().unwrap_or_default();
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
                if flags & 0x02 != 0 { frame.pop(); } // kwdefaults
                let mut defaults = Vec::new();
                if flags & 0x01 != 0 {
                    let default_tuple = frame.pop();
                    defaults = default_tuple.to_list().unwrap_or_default();
                }
                let code = match &code_obj.payload {
                    PyObjectPayload::Code(c) => *c.clone(),
                    _ => return Err(PyException::type_error(
                        "expected code object for MAKE_FUNCTION",
                    )),
                };
                let name_str = qualname.as_str().map(CompactString::from)
                    .unwrap_or_else(|| code.name.clone());
                let func = PyFunction {
                    name: name_str.clone(),
                    qualname: name_str,
                    code,
                    defaults,
                    kw_defaults: IndexMap::new(),
                    globals: frame.globals.clone(),
                    closure: closure_cells,
                    annotations: IndexMap::new(),
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
                while let Some(block) = frame.block_stack.last() {
                    if block.kind == BlockKind::Finally {
                        let handler = block.handler;
                        frame.block_stack.pop();
                        frame.pending_return = Some(value.clone());
                        frame.push(PyObject::none());
                        frame.ip = handler;
                        break;
                    } else {
                        frame.block_stack.pop();
                    }
                }
                if frame.pending_return.is_none() {
                    return Ok(Some(value));
                }
            }
            Opcode::ImportName => {
                let frame = self.vm_frame();
                let _fromlist = frame.pop();
                let level_obj = frame.pop();
                let level = level_obj.as_int().unwrap_or(0) as usize;
                let name = frame.code.names[instr.arg as usize].clone();
                let filename = frame.code.filename.clone();
                if let Some(module) = self.modules.get(&name) {
                    let module = module.clone();
                    self.vm_push(module);
                } else {
                    let resolved = if level > 0 {
                        ferrython_import::resolve_relative_import(&name, &filename, level)?
                    } else {
                        ferrython_import::resolve_module(&name, &filename)?
                    };
                    let module = match resolved {
                        ferrython_import::ResolvedModule::Builtin(m) => m,
                        ferrython_import::ResolvedModule::Source { code, name: mod_name } => {
                            let mod_globals = Arc::new(RwLock::new(IndexMap::new()));
                            let frame = Frame::new(code, mod_globals.clone(), self.builtins.clone());
                            self.call_stack.push(frame);
                            let _ = self.run_frame();
                            self.call_stack.pop();
                            let attrs = mod_globals.read().clone();
                            PyObject::module_with_attrs(mod_name, attrs)
                        }
                    };
                    self.modules.insert(name, module.clone());
                    self.vm_push(module);
                    return Ok(None);
                }
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
                    let all_names: Option<Vec<String>> = mod_data.attrs.get("__all__").and_then(|v| {
                        v.to_list().ok().map(|items| items.iter().map(|x| x.py_to_string()).collect())
                    });
                    let mut globals = frame.globals.write();
                    for (k, v) in &mod_data.attrs {
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
            Opcode::PopExcept => { self.vm_frame().pop_block(); }
            Opcode::EndFinally => {
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
                            PyObjectPayload::None => { frame.pop(); }
                            _ => {}
                        }
                    }
                }
            }
            Opcode::BeginFinally => {
                self.vm_frame().push(PyObject::none());
            }
            Opcode::RaiseVarargs => {
                let frame = self.vm_frame();
                let raise_exc = |exc: &PyObjectRef| -> PyException {
                    match &exc.payload {
                        PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                            PyException::new(kind.clone(), message.to_string())
                        }
                        PyObjectPayload::ExceptionType(kind) => {
                            PyException::new(kind.clone(), "")
                        }
                        PyObjectPayload::Instance(inst) => {
                            let kind = Self::find_exception_kind(&inst.class);
                            let msg = if let Some(m) = exc.get_attr("message") {
                                m.py_to_string()
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
                match instr.arg {
                    0 => {
                        // Bare raise: re-raise the currently active exception
                        if let Some(exc) = self.active_exception.clone() {
                            return Err(exc);
                        }
                        return Err(PyException::runtime_error("No active exception to re-raise"));
                    }
                    1 => {
                        let exc = frame.pop();
                        return Err(raise_exc(&exc));
                    }
                    2 => {
                        let cause = frame.pop();
                        let exc = frame.pop();
                        let mut py_exc = raise_exc(&exc);
                        let cause_exc = raise_exc(&cause);
                        py_exc.cause = Some(Box::new(cause_exc));
                        return Err(py_exc);
                    }
                    _ => return Err(PyException::runtime_error("bad RAISE_VARARGS arg")),
                }
            }
            Opcode::SetupWith => {
                let ctx_mgr = self.vm_pop();
                if let PyObjectPayload::Generator(gen_arc) = &ctx_mgr.payload {
                    let enter_result = match self.resume_generator(gen_arc, PyObject::none()) {
                        Ok(val) => val,
                        Err(e) if e.kind == ExceptionKind::StopIteration => PyObject::none(),
                        Err(e) => return Err(e),
                    };
                    let frame = self.vm_frame();
                    frame.push(ctx_mgr.clone());
                    frame.push_block(BlockKind::With, instr.arg as usize);
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
                    frame.push_block(BlockKind::With, instr.arg as usize);
                    frame.push(enter_result);
                }
            }
            Opcode::WithCleanupStart => {
                let tos = self.vm_frame().peek().clone();
                if matches!(tos.payload, PyObjectPayload::None) {
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
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(result);
                    }
                } else if matches!(tos.payload, PyObjectPayload::ExceptionType(_)) {
                    let exc_type = self.vm_pop();
                    let exc_val = if !self.vm_frame().stack.is_empty() { self.vm_pop() } else { PyObject::none() };
                    let exc_tb = if !self.vm_frame().stack.is_empty() { self.vm_pop() } else { PyObject::none() };
                    let exit_fn = self.vm_pop();
                    if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {}
                            Err(e) if e.kind == ExceptionKind::StopIteration => {}
                            Err(e) => return Err(e),
                        }
                        let f = self.vm_frame();
                        f.push(exc_type.clone());
                        f.push(PyObject::none());
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            exc_type.clone(), exc_val, exc_tb
                        ])?;
                        let f = self.vm_frame();
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
                    frame.push(PyObject::none());
                } else {
                    frame.push(exc_or_none);
                }
            }
            _ => unreachable!(),
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
                    1 => value.py_to_string(),
                    2 => value.repr(),
                    3 => value.py_to_string(),
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
                            value.py_to_string()
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

                if let PyObjectPayload::Generator(ref gen_arc) = sub_iter.payload {
                    let gen_arc = gen_arc.clone();
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
                            frame.push(PyObject::none());
                        }
                        Err(e) => return Err(e),
                    }
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
            _ => unreachable!(),
        }
        Ok(None)
    }
}
