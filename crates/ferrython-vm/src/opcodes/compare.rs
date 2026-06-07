//! Comparison operations: ==, !=, <, <=, >, >=, is, is not, in, not in

use crate::vm_truth::exception_kind_matches;
use crate::VirtualMachine;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::helpers::{
    instance_dict_as_hashkey_map, is_hidden_dict_key, partial_cmp_objects,
};
use ferrython_core::object::{
    has_descriptor_get, lookup_in_class_mro, CompareOp, FxHashKeyMap, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;

fn instance_class(obj: &PyObjectRef) -> Option<PyObjectRef> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        Some(inst.class.clone())
    } else {
        None
    }
}

fn has_dict_storage(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(inst)
        if inst.dict_storage.is_some() && !inst.attrs.read().contains_key("__weakref_ref__"))
}

fn is_weak_ref_instance(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(inst)
        if inst.attrs.read().contains_key("__weakref_ref__"))
}

fn is_deque_instance(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(inst)
        if inst.attrs.read().contains_key("__deque__"))
}

fn is_enum_member_instance(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(inst)
        if inst.attrs.read().contains_key("_name_") && inst.attrs.read().contains_key("_value_"))
}

fn is_decimal_instance(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(_))
        && obj
            .get_attr("__decimal__")
            .is_some_and(|marker| marker.is_truthy())
}

fn is_native_fraction_instance(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(inst)
        if inst.attrs.read().contains_key("__fraction__"))
}

fn is_numeric_fast_compare_operand(obj: &PyObjectRef) -> bool {
    matches!(
        obj.payload,
        PyObjectPayload::Bool(_) | PyObjectPayload::Int(_) | PyObjectPayload::Float(_)
    ) || is_decimal_instance(obj)
        || is_native_fraction_instance(obj)
}

fn should_use_numeric_fast_compare(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    if is_native_fraction_instance(a) && is_native_fraction_instance(b) {
        return false;
    }
    (is_decimal_instance(a) || is_decimal_instance(b))
        || ((is_native_fraction_instance(a) || is_native_fraction_instance(b))
            && is_numeric_fast_compare_operand(a)
            && is_numeric_fast_compare_operand(b))
}

fn builtin_value_compare_operands(
    a: &PyObjectRef,
    b: &PyObjectRef,
) -> Option<(PyObjectRef, PyObjectRef)> {
    if is_weak_ref_instance(a)
        || is_weak_ref_instance(b)
        || is_deque_instance(a)
        || is_deque_instance(b)
    {
        return None;
    }
    let a_value = match &a.payload {
        PyObjectPayload::Instance(inst) => inst.attrs.read().get("__builtin_value__").cloned(),
        _ => None,
    };
    let b_value = match &b.payload {
        PyObjectPayload::Instance(inst) => inst.attrs.read().get("__builtin_value__").cloned(),
        _ => None,
    };
    match (a_value, b_value) {
        (Some(left), Some(right)) => Some((left, right)),
        (Some(left), None)
            if matches!(
                b.payload,
                PyObjectPayload::Int(_)
                    | PyObjectPayload::Bool(_)
                    | PyObjectPayload::Float(_)
                    | PyObjectPayload::Str(_)
                    | PyObjectPayload::Tuple(_)
                    | PyObjectPayload::List(_)
                    | PyObjectPayload::Set(_)
                    | PyObjectPayload::FrozenSet(_)
            ) =>
        {
            Some((left, b.clone()))
        }
        (None, Some(right))
            if matches!(
                a.payload,
                PyObjectPayload::Int(_)
                    | PyObjectPayload::Bool(_)
                    | PyObjectPayload::Float(_)
                    | PyObjectPayload::Str(_)
                    | PyObjectPayload::Tuple(_)
                    | PyObjectPayload::List(_)
                    | PyObjectPayload::Set(_)
                    | PyObjectPayload::FrozenSet(_)
            ) =>
        {
            Some((a.clone(), right))
        }
        _ => None,
    }
}

fn class_is_strict_subclass(child: &PyObjectRef, parent: &PyObjectRef) -> bool {
    if PyObjectRef::ptr_eq(child, parent) {
        return false;
    }
    if let PyObjectPayload::Class(cd) = &child.payload {
        cd.mro.iter().any(|base| PyObjectRef::ptr_eq(base, parent))
    } else {
        false
    }
}

fn mapping_snapshot_for_compare(obj: &PyObjectRef) -> Option<FxHashKeyMap> {
    match &obj.payload {
        PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => Some(map.read().clone()),
        PyObjectPayload::InstanceDict(attrs) => Some(instance_dict_as_hashkey_map(attrs)),
        _ => None,
    }
}

fn vm_is_same_object(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    if PyObjectRef::ptr_eq(a, b) {
        return true;
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::InstanceDict(left), PyObjectPayload::InstanceDict(right)) => {
            std::rc::Rc::ptr_eq(left, right)
        }
        _ => false,
    }
}

impl VirtualMachine {
    fn unwrap_weak_proxy_for_compare(
        &mut self,
        obj: &PyObjectRef,
    ) -> Result<Option<PyObjectRef>, PyException> {
        let PyObjectPayload::Instance(inst) = &obj.payload else {
            return Ok(None);
        };
        let attrs = inst.attrs.read();
        if attrs.contains_key("__weakref_ref__") {
            return Ok(None);
        }
        let Some(target_fn) = attrs.get("__weakref_target__").cloned() else {
            return Ok(None);
        };
        drop(attrs);
        Ok(Some(self.call_object(target_fn, vec![])?))
    }

    fn compare_mapping_values_vm(
        &mut self,
        a: &PyObjectRef,
        b: &PyObjectRef,
    ) -> Result<Option<bool>, PyException> {
        if !matches!(
            (&a.payload, &b.payload),
            (PyObjectPayload::InstanceDict(_), _) | (_, PyObjectPayload::InstanceDict(_))
        ) {
            return Ok(None);
        }
        let Some(a_map) = mapping_snapshot_for_compare(a) else {
            return Ok(None);
        };
        let Some(b_map) = mapping_snapshot_for_compare(b) else {
            return Ok(None);
        };
        let a_visible: Vec<_> = a_map
            .iter()
            .filter(|(key, _)| !is_hidden_dict_key(key))
            .collect();
        let b_visible_len = b_map
            .iter()
            .filter(|(key, _)| !is_hidden_dict_key(key))
            .count();
        if a_visible.len() != b_visible_len {
            return Ok(Some(false));
        }
        for (key, left) in a_visible {
            let Some(right) = b_map.get(key) else {
                return Ok(Some(false));
            };
            let eq = self.compare_objects_for_mapping(left, right)?;
            if !eq {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    fn compare_objects_for_mapping(
        &mut self,
        left: &PyObjectRef,
        right: &PyObjectRef,
    ) -> Result<bool, PyException> {
        if PyObjectRef::ptr_eq(left, right) {
            return Ok(true);
        }
        if let Some(result) = self.call_eq_for_mapping(left, right)? {
            return Ok(result);
        }
        if let Some(result) = self.call_eq_for_mapping(right, left)? {
            return Ok(result);
        }
        Ok(left.compare(right, CompareOp::Eq)?.is_truthy())
    }

    fn call_eq_for_mapping(
        &mut self,
        obj: &PyObjectRef,
        other: &PyObjectRef,
    ) -> Result<Option<bool>, PyException> {
        if !matches!(&obj.payload, PyObjectPayload::Instance(_)) {
            return Ok(None);
        }
        let Some(eq_method) = obj.get_attr("__eq__") else {
            return Ok(None);
        };
        let result = self.call_object(eq_method, vec![other.clone()])?;
        if matches!(&result.payload, PyObjectPayload::NotImplemented) {
            Ok(None)
        } else {
            Ok(Some(result.is_truthy()))
        }
    }

    /// Derive a missing comparison from total_ordering root method
    fn derive_total_ordering(
        &mut self,
        a: &PyObjectRef,
        b: &PyObjectRef,
        dunder: &str,
        root: &str,
    ) -> Result<Option<PyObjectRef>, PyException> {
        // Helper: call a's dunder method via the VM
        let call_dunder = |vm: &mut Self,
                           obj: &PyObjectRef,
                           other: &PyObjectRef,
                           method: &str|
         -> Result<Option<bool>, PyException> {
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
        if dunder == root {
            return Ok(None);
        }

        match (root, dunder) {
            ("__lt__", "__le__") => {
                // a <= b  =  a < b or a == b
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    if lt {
                        return Ok(Some(PyObject::bool_val(true)));
                    }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(eq)));
                    }
                    return Ok(Some(PyObject::bool_val(false)));
                }
            }
            ("__lt__", "__gt__") => {
                // a > b  =  not (a < b) and not (a == b)
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    if lt {
                        return Ok(Some(PyObject::bool_val(false)));
                    }
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
                    if gt {
                        return Ok(Some(PyObject::bool_val(true)));
                    }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(eq)));
                    }
                    return Ok(Some(PyObject::bool_val(false)));
                }
            }
            ("__gt__", "__lt__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    if gt {
                        return Ok(Some(PyObject::bool_val(false)));
                    }
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

    pub(crate) fn exec_compare_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if instr.op == Opcode::LoadFastCompareConstJump {
            // Fallback path: decompose to LoadFast + LoadConst + CompareOp + PopJumpIfFalse
            let cmp_op = instr.arg >> 28;
            let local_idx = ((instr.arg >> 20) & 0xFF) as usize;
            let const_idx = ((instr.arg >> 12) & 0xFF) as usize;
            let jump_target = (instr.arg & 0xFFF) as usize;
            let frame = self.call_stack.last_mut().unwrap();
            let local = frame.locals[local_idx].clone().ok_or_else(|| {
                PyException::unbound_local_error(format!(
                    "local variable '{}' referenced before assignment",
                    frame
                        .code
                        .varnames
                        .get(local_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("?")
                ))
            })?;
            let c = frame.constant_cache[const_idx].clone();
            self.exec_compare_op(cmp_op, local, c)?;
            let v = self.vm_pop();
            let is_false = match &v.payload {
                PyObjectPayload::Bool(b) => !b,
                PyObjectPayload::None => true,
                PyObjectPayload::Int(PyInt::Small(n)) => *n == 0,
                _ => !self.vm_is_truthy(&v)?,
            };
            if is_false {
                self.call_stack.last_mut().unwrap().ip = jump_target;
            }
            return Ok(None);
        }
        if instr.op == Opcode::CompareOpPopJumpIfFalse {
            let cmp_op = instr.arg >> 24;
            let jump_target = (instr.arg & 0x00FF_FFFF) as usize;
            let (a, b) = self.vm_pop2();
            self.exec_compare_op(cmp_op, a, b)?;
            // Result is on stack; pop it and conditionally jump
            let v = self.vm_pop();
            let is_false = match &v.payload {
                PyObjectPayload::Bool(b) => !b,
                PyObjectPayload::None => true,
                PyObjectPayload::Int(PyInt::Small(n)) => *n == 0,
                _ => !self.vm_is_truthy(&v)?,
            };
            if is_false {
                self.call_stack.last_mut().unwrap().ip = jump_target;
            }
            return Ok(None);
        }
        let (a, b) = self.vm_pop2();
        self.exec_compare_op(instr.arg, a, b)
    }

    fn exec_compare_op(
        &mut self,
        op: u32,
        a: PyObjectRef,
        b: PyObjectRef,
    ) -> Result<Option<PyObjectRef>, PyException> {
        let cmp_to_key_compare = |vm: &mut Self,
                                  obj: &PyObjectRef,
                                  other: &PyObjectRef,
                                  method_name: &str|
         -> Result<Option<PyObjectRef>, PyException> {
            let cmp_op = match method_name {
                "__lt__" => CompareOp::Lt,
                "__le__" => CompareOp::Le,
                "__eq__" => CompareOp::Eq,
                "__ne__" => CompareOp::Ne,
                "__gt__" => CompareOp::Gt,
                "__ge__" => CompareOp::Ge,
                _ => return Ok(None),
            };
            let PyObjectPayload::Instance(left_inst) = &obj.payload else {
                return Ok(None);
            };
            let PyObjectPayload::Instance(right_inst) = &other.payload else {
                return Ok(None);
            };
            let cmp_func = {
                let PyObjectPayload::Class(cd) = &left_inst.class.payload else {
                    return Ok(None);
                };
                cd.namespace.read().get("__cmp_to_key_func__").cloned()
            };
            let Some(cmp_func) = cmp_func else {
                return Ok(None);
            };
            let Some(left_obj) = left_inst.attrs.read().get("obj").cloned() else {
                return Ok(None);
            };
            let Some(right_obj) = right_inst.attrs.read().get("obj").cloned() else {
                return Ok(None);
            };
            let cmp_result = vm.call_object(cmp_func, vec![left_obj, right_obj])?;
            let cmp_value = match &cmp_result.payload {
                PyObjectPayload::Bool(value) => PyObject::int(if *value { 1 } else { 0 }),
                _ => cmp_result,
            };
            let zero = PyObject::int(0);
            Ok(Some(cmp_value.compare(&zero, cmp_op)?))
        };
        let call_instance_dunder = |vm: &mut Self,
                                    obj: &PyObjectRef,
                                    other: &PyObjectRef,
                                    method_name: &str|
         -> Result<Option<PyObjectRef>, PyException> {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let Some(result) = cmp_to_key_compare(vm, obj, other, method_name)? {
                    return Ok(Some(result));
                }
                if inst.attrs.read().contains_key(method_name)
                    || inst.attrs.read().contains_key("__deque__")
                {
                    if let Some(method) = obj.get_attr(method_name) {
                        let result = vm.call_object(method, vec![other.clone()])?;
                        if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                            return Ok(Some(result));
                        }
                    }
                    return Ok(None);
                }
                match vm.call_plain_instance_dunder(obj, inst, method_name, vec![other.clone()]) {
                    Ok(Some(result)) => {
                        if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                            return Ok(Some(result));
                        }
                        return Ok(None);
                    }
                    Ok(None) => {}
                    Err(err) if matches!(err.kind, ExceptionKind::AttributeError) => {
                        return Ok(None);
                    }
                    Err(err) => return Err(err),
                }
                if let Some(method) = lookup_in_class_mro(&inst.class, method_name) {
                    let callable = if has_descriptor_get(&method) {
                        match vm.resolve_descriptor(&method, obj) {
                            Ok(resolved) => resolved,
                            Err(err) if matches!(err.kind, ExceptionKind::AttributeError) => {
                                return Ok(None);
                            }
                            Err(err) => return Err(err),
                        }
                    } else {
                        vm.bind_method(obj, method)
                    };
                    let result = vm.call_object(callable, vec![other.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(result));
                    }
                }
            }
            Ok(None)
        };
        let has_instance_dunder = |obj: &PyObjectRef, method_name: &str| -> bool {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if inst.attrs.read().contains_key(method_name)
                    || inst.attrs.read().contains_key("__deque__")
                {
                    return obj.get_attr(method_name).is_some();
                }
                lookup_in_class_mro(&inst.class, method_name).is_some()
            } else {
                false
            }
        };
        let call_instance_ne = |vm: &mut Self,
                                obj: &PyObjectRef,
                                other: &PyObjectRef|
         -> Result<Option<PyObjectRef>, PyException> {
            if has_instance_dunder(obj, "__ne__") {
                return call_instance_dunder(vm, obj, other, "__ne__");
            }
            if let Some(result) = call_instance_dunder(vm, obj, other, "__eq__")? {
                return Ok(Some(PyObject::bool_val(!result.is_truthy())));
            }
            Ok(None)
        };

        if let cmp @ 0..=5 = op {
            let unwrapped_a = self.unwrap_weak_proxy_for_compare(&a)?;
            let unwrapped_b = self.unwrap_weak_proxy_for_compare(&b)?;
            if unwrapped_a.is_some() || unwrapped_b.is_some() {
                let left = unwrapped_a.unwrap_or_else(|| a.clone());
                let right = unwrapped_b.unwrap_or_else(|| b.clone());
                return self.exec_compare_op(cmp, left, right);
            }

            if matches!(cmp, 0 | 1 | 4 | 5)
                && (matches!(&a.payload, PyObjectPayload::BoundMethod { .. })
                    || matches!(&b.payload, PyObjectPayload::BoundMethod { .. })
                    || matches!(&a.payload, PyObjectPayload::Function(_))
                    || matches!(&b.payload, PyObjectPayload::Function(_)))
            {
                return Err(PyException::type_error(format!(
                    "'{}' not supported between instances of '{}' and '{}'",
                    match cmp {
                        0 => "<",
                        1 => "<=",
                        4 => ">",
                        5 => ">=",
                        _ => unreachable!(),
                    },
                    a.type_name(),
                    b.type_name()
                )));
            }

            // Fast path: primitive types (int, float, str, bool) — skip MRO/dunder lookup
            match (&a.payload, &b.payload) {
                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                    let result = match cmp {
                        0 => x < y,
                        1 => x <= y,
                        2 => x == y,
                        3 => x != y,
                        4 => x > y,
                        _ => x >= y,
                    };
                    self.vm_push(PyObject::bool_val(result));
                    return Ok(None);
                }
                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                    let result = match cmp {
                        0 => x < y,
                        1 => x <= y,
                        2 => x == y,
                        3 => x != y,
                        4 => x > y,
                        _ => x >= y,
                    };
                    self.vm_push(PyObject::bool_val(result));
                    return Ok(None);
                }
                (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) if cmp == 2 || cmp == 3 => {
                    let eq = x == y;
                    self.vm_push(PyObject::bool_val(if cmp == 2 { eq } else { !eq }));
                    return Ok(None);
                }
                _ => {}
            }
            if matches!(cmp, 0 | 1 | 4 | 5) && should_use_numeric_fast_compare(&a, &b) {
                if let Some(ordering) = partial_cmp_objects(&a, &b) {
                    let result = match cmp {
                        0 => ordering == std::cmp::Ordering::Less,
                        1 => !matches!(ordering, std::cmp::Ordering::Greater),
                        4 => ordering == std::cmp::Ordering::Greater,
                        5 => !matches!(ordering, std::cmp::Ordering::Less),
                        _ => unreachable!(),
                    };
                    self.vm_push(PyObject::bool_val(result));
                    return Ok(None);
                }
            }
            let (dunder, rdunder) = match cmp {
                0 => ("__lt__", "__gt__"),
                1 => ("__le__", "__ge__"),
                2 => ("__eq__", "__eq__"),
                3 => ("__ne__", "__ne__"),
                4 => ("__gt__", "__lt__"),
                5 => ("__ge__", "__le__"),
                _ => unreachable!(),
            };
            if let Some((left, right)) = builtin_value_compare_operands(&a, &b) {
                let cmp_op = match cmp {
                    0 => CompareOp::Lt,
                    1 => CompareOp::Le,
                    2 => CompareOp::Eq,
                    3 => CompareOp::Ne,
                    4 => CompareOp::Gt,
                    5 => CompareOp::Ge,
                    _ => unreachable!(),
                };
                let result = left.compare(&right, cmp_op)?;
                self.vm_push(result);
                return Ok(None);
            }
            let right_is_subclass = match (instance_class(&a), instance_class(&b)) {
                (Some(left), Some(right)) => class_is_strict_subclass(&right, &left),
                _ => false,
            };
            if cmp == 3 {
                if right_is_subclass {
                    if let Some(result) = call_instance_ne(self, &b, &a)? {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
                if let Some(result) = call_instance_ne(self, &a, &b)? {
                    self.vm_push(result);
                    return Ok(None);
                }
                if !right_is_subclass {
                    if let Some(result) = call_instance_ne(self, &b, &a)? {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            } else {
                if right_is_subclass {
                    if let Some(result) = call_instance_dunder(self, &b, &a, rdunder)? {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
                if let Some(result) = call_instance_dunder(self, &a, &b, dunder)? {
                    self.vm_push(result);
                    return Ok(None);
                }
                if let PyObjectPayload::Instance(inst) = &a.payload {
                    // total_ordering fallback: derive missing comparisons from root
                    if let Some(root_marker) =
                        lookup_in_class_mro(&inst.class, "__total_ordering_root__")
                    {
                        let root = root_marker.py_to_string();
                        if let Some(result) = self.derive_total_ordering(&a, &b, dunder, &root)? {
                            self.vm_push(result);
                            return Ok(None);
                        }
                    }
                }
                if !right_is_subclass {
                    if let Some(result) = call_instance_dunder(self, &b, &a, rdunder)? {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
            if matches!(cmp, 2 | 3) {
                if let Some(eq) = self.compare_mapping_values_vm(&a, &b)? {
                    self.vm_push(PyObject::bool_val(if cmp == 2 { eq } else { !eq }));
                    return Ok(None);
                }
            }
            if matches!(cmp, 2 | 3)
                && ((has_dict_storage(&a) && matches!(&b.payload, PyObjectPayload::Dict(_)))
                    || (matches!(&a.payload, PyObjectPayload::Dict(_)) && has_dict_storage(&b))
                    || (has_dict_storage(&a) && has_dict_storage(&b)))
            {
                let cmp_op = if cmp == 2 {
                    CompareOp::Eq
                } else {
                    CompareOp::Ne
                };
                let result = a.compare(&b, cmp_op)?;
                self.vm_push(result);
                return Ok(None);
            }
            // Dataclass auto-equality fallback
            if cmp == 2 || cmp == 3 {
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) =
                    (&a.payload, &b.payload)
                {
                    let cls_a = &inst_a.class;
                    if cls_a.get_attr("__dataclass__").is_some() {
                        if let Some(fields) = cls_a.get_attr("__dataclass_fields__") {
                            let field_names =
                                crate::vm_dataclass_utils::extract_field_names(&fields);
                            if !field_names.is_empty() {
                                let attrs_a = inst_a.attrs.read();
                                let attrs_b = inst_b.attrs.read();
                                let mut eq = true;
                                for name in &field_names {
                                    let va = attrs_a.get(name.as_str());
                                    let vb = attrs_b.get(name.as_str());
                                    match (va, vb) {
                                        (Some(x), Some(y)) => {
                                            if let Ok(r) = x.compare(y, CompareOp::Eq) {
                                                if !r.is_truthy() {
                                                    eq = false;
                                                    break;
                                                }
                                            } else {
                                                eq = false;
                                                break;
                                            }
                                        }
                                        _ => {
                                            eq = false;
                                            break;
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
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) =
                    (&a.payload, &b.payload)
                {
                    if inst_a.class.get_attr("__namedtuple__").is_some()
                        && inst_b.class.get_attr("__namedtuple__").is_some()
                    {
                        let ta = inst_a.attrs.read().get("_tuple").cloned();
                        let tb = inst_b.attrs.read().get("_tuple").cloned();
                        if let (Some(tup_a), Some(tup_b)) = (ta, tb) {
                            let result = tup_a.compare(&tup_b, CompareOp::Eq)?;
                            let val = if cmp == 2 {
                                result.is_truthy()
                            } else {
                                !result.is_truthy()
                            };
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
                                let val = if cmp == 2 {
                                    result.is_truthy()
                                } else {
                                    !result.is_truthy()
                                };
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
                                let val = if cmp == 2 {
                                    result.is_truthy()
                                } else {
                                    !result.is_truthy()
                                };
                                self.vm_push(PyObject::bool_val(val));
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            // IntEnum/enum value-based comparison fallback
            if !is_weak_ref_instance(&a)
                && !is_weak_ref_instance(&b)
                && is_enum_member_instance(&a)
                && is_enum_member_instance(&b)
            {
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) =
                    (&a.payload, &b.payload)
                {
                    let va = inst_a.attrs.read().get("value").cloned();
                    let vb = inst_b.attrs.read().get("value").cloned();
                    if let (Some(av), Some(bv)) = (va, vb) {
                        if matches!(
                            av.payload,
                            PyObjectPayload::Int(_) | PyObjectPayload::Float(_)
                        ) && matches!(
                            bv.payload,
                            PyObjectPayload::Int(_) | PyObjectPayload::Float(_)
                        ) {
                            let cmp_op = match cmp {
                                0 => CompareOp::Lt,
                                1 => CompareOp::Le,
                                2 => CompareOp::Eq,
                                3 => CompareOp::Ne,
                                4 => CompareOp::Gt,
                                5 => CompareOp::Ge,
                                _ => unreachable!(),
                            };
                            let result = av.compare(&bv, cmp_op)?;
                            self.vm_push(result);
                            return Ok(None);
                        }
                    }
                }
            }
            // IntEnum vs plain int/float comparison
            if !is_weak_ref_instance(&a) && !is_weak_ref_instance(&b) {
                let (enum_val, other) = if is_enum_member_instance(&a) {
                    match &a.payload {
                        PyObjectPayload::Instance(inst) => {
                            (inst.attrs.read().get("value").cloned(), Some(&b))
                        }
                        _ => (None, None),
                    }
                } else if is_enum_member_instance(&b) {
                    match &b.payload {
                        PyObjectPayload::Instance(inst) => {
                            (inst.attrs.read().get("value").cloned(), Some(&a))
                        }
                        _ => (None, None),
                    }
                } else {
                    (None, None)
                };
                if let (Some(ev), Some(ov)) = (enum_val, other) {
                    if matches!(
                        ev.payload,
                        PyObjectPayload::Int(_) | PyObjectPayload::Float(_)
                    ) && matches!(
                        ov.payload,
                        PyObjectPayload::Int(_) | PyObjectPayload::Float(_)
                    ) {
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
                            _ => unreachable!(),
                        };
                        let result = left.compare(&right, cmp_op)?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
            if matches!(cmp, 2 | 3)
                && (matches!(&a.payload, PyObjectPayload::Instance(_))
                    || matches!(&b.payload, PyObjectPayload::Instance(_)))
            {
                let same = a.is_same(&b);
                self.vm_push(PyObject::bool_val(if cmp == 2 { same } else { !same }));
                return Ok(None);
            }
            if matches!(cmp, 0 | 1 | 4 | 5)
                && (matches!(&a.payload, PyObjectPayload::Instance(_))
                    || matches!(&b.payload, PyObjectPayload::Instance(_)))
            {
                let cmp_op = match cmp {
                    0 => CompareOp::Lt,
                    1 => CompareOp::Le,
                    4 => CompareOp::Gt,
                    5 => CompareOp::Ge,
                    _ => unreachable!(),
                };
                if let Ok(result) = a.compare(&b, cmp_op) {
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
                return Err(PyException::type_error(format!(
                    "'{}' not supported between instances of '{}' and '{}'",
                    match cmp {
                        0 => "<",
                        1 => "<=",
                        4 => ">",
                        5 => ">=",
                        _ => unreachable!(),
                    },
                    a.type_name(),
                    b.type_name()
                )));
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
                    let val = if op == 6 {
                        r.is_truthy()
                    } else {
                        !r.is_truthy()
                    };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
            if let PyObjectPayload::Instance(inst) = &b.payload {
                if inst.attrs.read().contains_key("__chainmap__") {
                    if let Some(maps_obj) = b.get_attr("maps") {
                        let maps = maps_obj.to_list()?;
                        let mut found = false;
                        for mapping in maps {
                            match mapping.get_item(&a) {
                                Ok(_) => {
                                    found = true;
                                    break;
                                }
                                Err(e) if e.kind == ExceptionKind::KeyError => continue,
                                Err(e) => return Err(e),
                            }
                        }
                        let val = if op == 6 { found } else { !found };
                        self.vm_push(PyObject::bool_val(val));
                        return Ok(None);
                    }
                }
                // Check for user-defined __contains__ in the class (including dict subclasses)
                let custom_contains = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    if inst.attrs.read().contains_key("__deque__") {
                        None
                    } else {
                        cd.namespace.read().get("__contains__").cloned()
                    }
                } else {
                    None
                };
                if let Some(method) = custom_contains {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 {
                        r.is_truthy()
                    } else {
                        !r.is_truthy()
                    };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Dict subclass: use native contains() directly
                if inst.dict_storage.is_some() && !inst.attrs.read().contains_key("__chainmap__") {
                    let val = if op == 6 {
                        b.contains(&a)?
                    } else {
                        !b.contains(&a)?
                    };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                if let Some(method) = b.get_attr("__contains__") {
                    let r = self.call_object(method, vec![a])?;
                    let val = if op == 6 {
                        r.is_truthy()
                    } else {
                        !r.is_truthy()
                    };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Fallback: iterate via __iter__ (CPython behavior)
                if let Some(iter_method) = b.get_attr("__iter__") {
                    let iterator = self.call_object(iter_method, vec![])?;
                    let iterator = Self::ensure_iterator_result(&b, iterator)?;
                    let mut found = false;
                    loop {
                        match self.vm_iter_next(&iterator)? {
                            Some(item) => {
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
                            Err(e)
                                if e.kind == ferrython_core::error::ExceptionKind::IndexError =>
                            {
                                break
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    let val = if op == 6 { found } else { !found };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
            // Module with __contains__ (e.g., os.environ)
            if let PyObjectPayload::Module(ref md) = &b.payload {
                let contains_fn = md.attrs.read().get("__contains__").cloned();
                if let Some(method) = contains_fn {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 {
                        r.is_truthy()
                    } else {
                        !r.is_truthy()
                    };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                if b.get_attr("__iter__").is_some() || b.get_attr("__next__").is_some() {
                    let iterator = crate::builtins::get_iter_from_obj_pub(&b)?;
                    let mut found = false;
                    loop {
                        match self.vm_iter_next(&iterator)? {
                            Some(item) => {
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
            8 => PyObject::bool_val(vm_is_same_object(&a, &b)),
            9 => PyObject::bool_val(!vm_is_same_object(&a, &b)),
            10 => {
                let validate_handler = |handler: &PyObjectRef| -> Result<(), PyException> {
                    match &handler.payload {
                        PyObjectPayload::ExceptionType(_) => Ok(()),
                        PyObjectPayload::Class(_) if Self::is_exception_class(handler) => Ok(()),
                        PyObjectPayload::Instance(inst)
                            if Self::is_exception_class(&inst.class) =>
                        {
                            let mut exc = PyException::type_error(
                                "catching classes that do not inherit from BaseException is not allowed",
                            );
                            exc.original = Some(PyObject::exception_instance(
                                ExceptionKind::TypeError,
                                exc.message.clone(),
                            ));
                            Err(exc)
                        }
                        _ => {
                            let mut exc = PyException::type_error(
                                "catching classes that do not inherit from BaseException is not allowed",
                            );
                            exc.original = Some(PyObject::exception_instance(
                                ExceptionKind::TypeError,
                                exc.message.clone(),
                            ));
                            Err(exc)
                        }
                    }
                };
                let match_one = |a_item: &PyObjectRef, b_item: &PyObjectRef| -> bool {
                    // Case 1: Both are user-defined Class payloads
                    if let PyObjectPayload::Class(cls_a) = &a_item.payload {
                        if let PyObjectPayload::Class(cls_b) = &b_item.payload {
                            // Check name match or MRO/bases membership
                            if cls_a.name == cls_b.name {
                                return true;
                            }
                            for base in &cls_a.mro {
                                if let PyObjectPayload::Class(bc) = &base.payload {
                                    if bc.name == cls_b.name {
                                        return true;
                                    }
                                }
                            }
                            for base in &cls_a.bases {
                                if let PyObjectPayload::Class(bc) = &base.payload {
                                    if bc.name == cls_b.name {
                                        return true;
                                    }
                                }
                            }
                            return false;
                        }
                        // Raised is user-defined class, handler is builtin ExceptionType
                        if let PyObjectPayload::ExceptionType(kind_b) = &b_item.payload {
                            // Check all exception kinds in the MRO, not just the first one.
                            // This handles multiple inheritance like BadRequestKeyError(BadRequest, KeyError)
                            // where we need to match against KeyError even though BadRequest comes first.
                            return Self::any_exception_kind_matches(a_item, kind_b);
                        }
                        return false;
                    }
                    // Case 2: Raised is a builtin ExceptionType
                    if let PyObjectPayload::ExceptionType(kind_a) = &a_item.payload {
                        return match &b_item.payload {
                            // Handler is also builtin ExceptionType
                            PyObjectPayload::ExceptionType(kind_b) => {
                                exception_kind_matches(kind_a, kind_b)
                            }
                            // Handler is a user-defined Class: only match if the
                            // class name directly names this builtin exception kind
                            // (e.g., class named "ValueError" catches ValueError).
                            // A user-defined class like SkipTest(Exception) must NOT
                            // catch builtin exceptions just because its base is Exception.
                            PyObjectPayload::Class(cls_b) => {
                                if let Some(kind_b) = ExceptionKind::from_name(&cls_b.name) {
                                    exception_kind_matches(kind_a, &kind_b)
                                } else {
                                    false
                                }
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
                    PyObjectPayload::Tuple(items) => {
                        let mut matched = false;
                        for item in items.iter() {
                            validate_handler(item)?;
                            if match_one(&a, item) {
                                matched = true;
                            }
                        }
                        matched
                    }
                    _ => {
                        validate_handler(&b)?;
                        match_one(&a, &b)
                    }
                };
                PyObject::bool_val(matched)
            }
            _ => return Err(PyException::runtime_error("invalid compare op")),
        };
        self.vm_push(result);
        Ok(None)
    }
}
