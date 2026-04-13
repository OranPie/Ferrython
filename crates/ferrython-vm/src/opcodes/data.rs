use crate::builtins;
use crate::frame::ScopeKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use ferrython_core::intern;
use std::rc::Rc;

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
                    ScopeKind::Module => {
                        frame.globals.write().insert(name, value);
                        crate::frame::bump_globals_version();
                    }
                    ScopeKind::Class => { frame.local_names_insert(name, value); }
                    ScopeKind::Function => { frame.local_names_insert(name, value); }
                }
            }
            Opcode::DeleteName => {
                let name = frame.code.names[instr.arg as usize].clone();
                let old = frame.local_names_get(name.as_str())
                    .or_else(|| frame.globals.read().get(name.as_str()).cloned());
                frame.local_names_remove(name.as_str());
                frame.globals.write().shift_remove(name.as_str());
                crate::frame::bump_globals_version();
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
                if let Some(v) = frame.local_names_get(name.as_str()) {
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
                let idx = instr.arg as usize;
                let ver = crate::frame::globals_version();
                // Check inline cache
                if frame.global_cache_version == ver {
                    if let Some(ref cache) = frame.global_cache {
                        if let Some(ref v) = cache[idx] {
                            frame.push(v.clone());
                            return Ok(None);
                        }
                    }
                } else if frame.global_cache.is_some() {
                    // Version mismatch — invalidate (clone-on-write if shared)
                    let cache = Rc::make_mut(frame.global_cache.as_mut().unwrap());
                    for slot in cache.iter_mut() { *slot = None; }
                    frame.global_cache_version = ver;
                }
                let name = &frame.code.names[idx];
                let from_globals = frame.globals.read().get(name.as_str()).cloned();
                let resolved = if let Some(v) = from_globals {
                    v
                } else if let Some(v) = frame.builtins.get(name.as_str()) {
                    v.clone()
                } else {
                    return Err(PyException::name_error(format!(
                        "name '{}' is not defined", name
                    )));
                };
                // Lazily allocate and populate cache (clone-on-write if shared)
                let cache = frame.global_cache.get_or_insert_with(|| {
                    Rc::new(vec![None; frame.code.names.len()])
                });
                Rc::make_mut(cache)[idx] = Some(resolved.clone());
                frame.global_cache_version = ver;
                frame.push(resolved);
            }
            Opcode::StoreGlobal => {
                let name = frame.code.names[instr.arg as usize].clone();
                let value = frame.pop();
                frame.globals.write().insert(name, value);
                crate::frame::bump_globals_version();
            }
            Opcode::DeleteGlobal => {
                let name = &frame.code.names[instr.arg as usize];
                frame.globals.write().shift_remove(name.as_str());
                crate::frame::bump_globals_version();
            }
            // Superinstructions (fallback path — normally handled inline in dispatch loop)
            Opcode::LoadFastLoadFast => {
                let idx1 = (instr.arg >> 16) as usize;
                let idx2 = (instr.arg & 0xFFFF) as usize;
                let v1 = frame.get_local(idx1).cloned().ok_or_else(|| PyException::name_error(format!(
                    "local variable '{}' referenced before assignment",
                    frame.code.varnames.get(idx1).map(|s| s.as_str()).unwrap_or("?")
                )))?;
                let v2 = frame.get_local(idx2).cloned().ok_or_else(|| PyException::name_error(format!(
                    "local variable '{}' referenced before assignment",
                    frame.code.varnames.get(idx2).map(|s| s.as_str()).unwrap_or("?")
                )))?;
                frame.push(v1);
                frame.push(v2);
            }
            Opcode::LoadFastLoadConst => {
                let fast_idx = (instr.arg >> 16) as usize;
                let const_idx = (instr.arg & 0xFFFF) as usize;
                let v = frame.get_local(fast_idx).cloned().ok_or_else(|| PyException::name_error(format!(
                    "local variable '{}' referenced before assignment",
                    frame.code.varnames.get(fast_idx).map(|s| s.as_str()).unwrap_or("?")
                )))?;
                frame.push(v);
                frame.push(frame.constant_cache[const_idx].clone());
            }
            Opcode::StoreFastLoadFast => {
                let store_idx = (instr.arg >> 16) as usize;
                let load_idx = (instr.arg & 0xFFFF) as usize;
                let val = frame.pop();
                frame.set_local(store_idx, val);
                let v = frame.get_local(load_idx).cloned().ok_or_else(|| PyException::name_error(format!(
                    "local variable '{}' referenced before assignment",
                    frame.code.varnames.get(load_idx).map(|s| s.as_str()).unwrap_or("?")
                )))?;
                frame.push(v);
            }
            Opcode::LoadConstStoreFast => {
                let const_idx = (instr.arg >> 16) as usize;
                let store_idx = (instr.arg & 0xFFFF) as usize;
                let val = frame.constant_cache[const_idx].clone();
                frame.set_local(store_idx, val);
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
        // Transparent weakref.proxy delegation: upgrade and substitute referent
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    let referent = (nc.func)(&[])?;
                    return self.exec_load_attr(name, referent);
                }
            }
        }

        // __getattribute__ override: called before normal lookup
        // Fast-path: skip MRO scan if the class doesn't override __getattribute__
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let has_ga = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.has_getattribute
            } else {
                false
            };
            if has_ga {
                if let Some(ga) = lookup_in_class_mro(&inst.class, "__getattribute__") {
                    if matches!(&ga.payload, PyObjectPayload::Function(_)) {
                        let method = PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: ga,
                            }
                        });
                        let name_arg = PyObject::str_val(intern::intern_or_new(name.as_str()));
                        match self.call_object(method, vec![name_arg]) {
                            Ok(result) => {
                                self.vm_push(result);
                                return Ok(None);
                            }
                            Err(e) if e.kind == ExceptionKind::AttributeError => {
                                // Fall through to __getattr__
                                if let Some(ga2) = obj.get_attr("__getattr__") {
                                    let name_arg2 = PyObject::str_val(intern::intern_or_new(name.as_str()));
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
        }
        // For Instance objects, check if the instance's own class defines
        // this attribute as a method (Function). Class-level methods act as
        // data descriptors and take precedence over instance __dict__,
        // ensuring subclass method overrides work correctly even when a
        // parent __init__ installs same-named closures as instance attrs.
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                if let Some(class_val) = cd.namespace.read().get(name.as_str()).cloned() {
                    if matches!(&class_val.payload,
                        PyObjectPayload::Function(_) |
                        PyObjectPayload::NativeFunction(_) |
                        PyObjectPayload::NativeClosure { .. } |
                        PyObjectPayload::Property(_)
                    ) {
                        if let PyObjectPayload::Property(pd) = &class_val.payload {
                            if let Some(getter) = &pd.fget {
                                let getter = crate::builtins::unwrap_abstract_fget(getter);
                                let result = self.call_object(getter, vec![obj.clone()])?;
                                self.vm_push(result);
                                return Ok(None);
                            }
                        } else {
                            // Wrap as BoundMethod
                            let bound = PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: class_val,
                                }
                            });
                            self.vm_push(bound);
                            return Ok(None);
                        }
                    }
                }
            }
        }
        match obj.get_attr(name) {
            Some(v) => {
                if let PyObjectPayload::Property(pd) = &v.payload {
                    // On class-level access (e.g. MyClass.prop), return the property
                    // object itself — don't invoke pd.fget.
                    if matches!(&obj.payload, PyObjectPayload::Class(_)) {
                        self.vm_push(v);
                    } else if let Some(getter) = &pd.fget {
                        // Unwrap abstract marker if present (@property @abstractmethod)
                        let getter = crate::builtins::unwrap_abstract_fget(getter);
                        // When property is accessed via super() proxy, pass the
                        // underlying instance to pd.fget, not the Super wrapper.
                        let receiver = if let PyObjectPayload::Super { instance, .. } = &obj.payload {
                            instance.clone()
                        } else {
                            obj
                        };
                        let result = self.call_object(getter, vec![receiver])?;
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
                            inst.attrs.write().insert(intern::intern_or_new(name.as_str()), result.clone());
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
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: v.clone(),
                                method: get_method,
                            }
                        })
                    };
                    let result = self.call_object(get_method_bound, vec![instance_arg, owner_arg])?;
                    self.vm_push(result);
                } else if matches!(&obj.payload, PyObjectPayload::Module(_))
                    && matches!(&v.payload, PyObjectPayload::NativeFunction(_))
                    && obj.get_attr("_bind_methods").is_some()
                {
                    self.vm_push(PyObjectRef::new(PyObject {
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
                    PyObjectPayload::NativeFunction(nf) => Some(nf.name.as_str()),
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
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    // Look up __getattr__ through class MRO and apply descriptor protocol
                    if let Some(ga_raw) = lookup_in_class_mro(&inst.class, "__getattr__") {
                        let ga = if has_descriptor_get(&ga_raw) {
                            // Descriptor (e.g. _ProxyLookup): invoke __get__ to bind
                            if let Some(get_method) = ga_raw.get_attr("__get__") {
                                let owner = inst.class.clone();
                                let get_bound = if matches!(&get_method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    get_method
                                } else {
                                    PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: ga_raw.clone(),
                                            method: get_method,
                                        }
                                    })
                                };
                                self.call_object(get_bound, vec![obj.clone(), owner])?
                            } else {
                                ga_raw
                            }
                        } else if matches!(&ga_raw.payload, PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                            // Regular function: bind as method
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: ga_raw,
                                }
                            })
                        } else {
                            ga_raw
                        };
                        let name_arg = PyObject::str_val(intern::intern_or_new(name.as_str()));
                        let result = self.call_object(ga, vec![name_arg])?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
                // PEP 562: module-level __getattr__
                if let PyObjectPayload::Module(_) = &obj.payload {
                    if let Some(ga) = obj.get_attr("__getattr__") {
                        let name_arg = PyObject::str_val(intern::intern_or_new(name.as_str()));
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
                if let PyObjectPayload::Property(pd) = &desc.payload {
                    if let Some(setter) = &pd.fset {
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
                            PyObjectRef::new(PyObject {
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
                        let method = PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: sa,
                            }
                        });
                        let name_arg = PyObject::str_val(name.clone());
                        self.call_object(method, vec![name_arg, value])?;
                        return Ok(None);
                    }
                    PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => {
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
                // Check __slots__ restriction via ClassData.slots field
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    if let Some(allowed) = cd.collect_all_slots() {
                        // If __dict__ is in slots, allow any attribute
                        if !allowed.iter().any(|s| s.as_str() == "__dict__") {
                            if !allowed.iter().any(|s| s.as_str() == name.as_str()) {
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'",
                                    cd.name, name
                                )));
                            }
                        }
                    }
                }
                // Special case: __dict__ assignment replaces instance attrs
                if name.as_str() == "__dict__" {
                    match &value.payload {
                        PyObjectPayload::Dict(map) => {
                            let mut attrs = inst.attrs.write();
                            attrs.clear();
                            for (k, v) in map.read().iter() {
                                if let HashableKey::Str(s) = k {
                                    attrs.insert(s.as_ref().clone(), v.clone());
                                }
                            }
                        }
                        PyObjectPayload::InstanceDict(other_attrs) => {
                            let mut attrs = inst.attrs.write();
                            attrs.clear();
                            for (k, v) in other_attrs.read().iter() {
                                attrs.insert(k.clone(), v.clone());
                            }
                        }
                        _ => {
                            inst.attrs.write().insert(name.clone(), value);
                        }
                    }
                } else {
                    inst.attrs.write().insert(name.clone(), value);
                }
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
            // Native functions: silently accept attribute assignment (common in decorators)
            PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) |
            PyObjectPayload::BuiltinFunction(_) => {
                // No persistent storage, but don't error — many decorators set __wrapped__ etc.
            }
            PyObjectPayload::ExceptionInstance(ei) => {
                ei.ensure_attrs().write().insert(name.clone(), value);
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
                    if let PyObjectPayload::Property(pd) = &class_attr.payload {
                        if let Some(fdel_fn) = &pd.fdel {
                            let bound = PyObjectRef::new(PyObject {
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
                                PyObjectRef::new(PyObject {
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
                            let method = PyObjectRef::new(PyObject {
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
                        let method = PyObjectRef::new(PyObject {
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
            PyObjectPayload::Module(md) => {
                md.attrs.write().shift_remove(name.as_str());
            }
            PyObjectPayload::Function(f) => {
                if f.attrs.write().swap_remove(name.as_str()).is_none() {
                    return Err(PyException::attribute_error(format!(
                        "'function' object has no attribute '{}'", name)));
                }
            }
            _ => return Err(PyException::attribute_error(format!(
                "'{}' object does not support attribute deletion", obj.type_name()))),
        }
        Ok(None)
    }
}
