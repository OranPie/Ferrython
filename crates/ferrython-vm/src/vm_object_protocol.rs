//! VM object protocol helpers — dunder lookup, descriptors, hashing, and iterator validation.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    has_descriptor_get, InstanceData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_GETATTR, CLASS_FLAG_HAS_GETATTRIBUTE,
};
use ferrython_core::types::{HashableKey, PyFunction, PyInt};

impl VirtualMachine {
    /// Install thread-local __hash__ and __eq__ dispatch callbacks for HashableKey.
    /// Called once at VM creation so all set/dict operations can resolve custom hashing.
    pub(crate) fn install_hash_eq_dispatch(&mut self) {
        let vm_ptr = self as *mut VirtualMachine;
        ferrython_core::types::set_eq_dispatch(move |a: &PyObjectRef, b: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr };
            if let PyObjectPayload::Instance(inst) = &a.payload {
                if let Some(result) = Self::try_inline_plain_attr_pair_eq(a, inst, b) {
                    return Ok(Some(result));
                }
                if let Some(result) =
                    vm.call_plain_instance_dunder(a, inst, "__eq__", vec![b.clone()])?
                {
                    if matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(None);
                    }
                    return Ok(Some(result.is_truthy()));
                }
                if let Some(result) = Self::compare_builtin_value_subclass(a, inst, b)? {
                    return Ok(Some(result));
                }
            }
            if let Some(eq_method) = a.get_attr("__eq__") {
                let result = vm.call_object(eq_method, vec![b.clone()])?;
                if matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Ok(None);
                }
                return Ok(Some(result.is_truthy()));
            }
            Ok(None)
        });

        let vm_ptr2 = self as *mut VirtualMachine;
        ferrython_core::types::set_hash_dispatch(move |obj: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr2 };
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                let is_weak_ref_like = {
                    let attrs = inst.attrs.read();
                    attrs.contains_key("__weakref_ref__") || attrs.contains_key("__weakmethod__")
                };
                if !is_weak_ref_like && Self::class_blocks_hash(&inst.class) {
                    return Err(PyException::type_error(format!(
                        "unhashable type: '{}'",
                        obj.type_name()
                    )));
                }
                if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if matches!(&value.payload, PyObjectPayload::FrozenSet(_)) {
                        if let Ok(key) = value.to_hashable_key() {
                            use std::collections::hash_map::DefaultHasher;
                            use std::hash::{Hash, Hasher};
                            let mut hasher = DefaultHasher::new();
                            key.hash(&mut hasher);
                            return Ok(Some(hasher.finish() as i64));
                        }
                    }
                }
            }
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let Some(result) =
                    vm.call_plain_instance_dunder(obj, inst, "__hash__", Vec::new())?
                {
                    return Ok(Some(result.as_int().unwrap_or(0)));
                }
            }
            if let Some(hash_method) = obj.get_attr("__hash__") {
                let call_args =
                    if matches!(&hash_method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![obj.clone()]
                    };
                let result = vm.call_object(hash_method, call_args)?;
                return Ok(Some(result.as_int().unwrap_or(0)));
            }
            Ok(None)
        });

        let vm_ptr3 = self as *mut VirtualMachine;
        ferrython_core::object::register_vm_call_dispatch(
            move |func: PyObjectRef, args: Vec<PyObjectRef>| {
                let vm = unsafe { &mut *vm_ptr3 };
                vm.call_object(func, args)
            },
        );
        let vm_ptr4 = self as *mut VirtualMachine;
        ferrython_core::object::register_vm_call_kw_dispatch(
            move |func: PyObjectRef,
                  args: Vec<PyObjectRef>,
                  kwargs: Vec<(CompactString, PyObjectRef)>| {
                let vm = unsafe { &mut *vm_ptr4 };
                vm.call_object_kw(func, args, kwargs)
            },
        );
    }

    pub(crate) fn is_exception_class(cls: &PyObjectRef) -> bool {
        if matches!(&cls.payload, PyObjectPayload::ExceptionType(_)) {
            return true;
        }
        if let PyObjectPayload::Class(cd) = &cls.payload {
            for base in &cd.bases {
                if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                    return true;
                }
                if Self::is_exception_class(base) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn class_blocks_hash(cls: &PyObjectRef) -> bool {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if let Some(value) = cd.namespace.read().get("__hash__") {
                return matches!(&value.payload, PyObjectPayload::None);
            }
            if cd.namespace.read().contains_key("__eq__") {
                return true;
            }
            for base in &cd.mro {
                if let PyObjectPayload::Class(base_cd) = &base.payload {
                    if let Some(value) = base_cd.namespace.read().get("__hash__") {
                        return matches!(&value.payload, PyObjectPayload::None);
                    }
                    if base_cd.namespace.read().contains_key("__eq__") {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn compare_builtin_value_subclass(
        _left: &PyObjectRef,
        left_inst: &InstanceData,
        right: &PyObjectRef,
    ) -> PyResult<Option<bool>> {
        let Some(left_value) = left_inst.attrs.read().get("__builtin_value__").cloned() else {
            return Ok(None);
        };
        let right_value = if let PyObjectPayload::Instance(right_inst) = &right.payload {
            right_inst
                .attrs
                .read()
                .get("__builtin_value__")
                .cloned()
                .unwrap_or_else(|| right.clone())
        } else {
            right.clone()
        };
        match (&left_value.payload, &right_value.payload) {
            (PyObjectPayload::List(left_items), PyObjectPayload::List(right_items)) => {
                let left_items = left_items.read();
                let right_items = right_items.read();
                if left_items.len() != right_items.len() {
                    return Ok(Some(false));
                }
                Ok(Some(left_items.iter().zip(right_items.iter()).all(
                    |(a, b)| {
                        a.compare(b, ferrython_core::object::CompareOp::Eq)
                            .map_or(false, |v| v.is_truthy())
                    },
                )))
            }
            (PyObjectPayload::Tuple(left_items), PyObjectPayload::Tuple(right_items)) => {
                if left_items.len() != right_items.len() {
                    return Ok(Some(false));
                }
                Ok(Some(left_items.iter().zip(right_items.iter()).all(
                    |(a, b)| {
                        a.compare(b, ferrython_core::object::CompareOp::Eq)
                            .map_or(false, |v| v.is_truthy())
                    },
                )))
            }
            _ => {
                if std::mem::discriminant(&left_value.payload)
                    == std::mem::discriminant(&right_value.payload)
                {
                    let result =
                        left_value.compare(&right_value, ferrython_core::object::CompareOp::Eq)?;
                    Ok(Some(result.is_truthy()))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub(crate) fn ensure_iterator_result(
        owner: &PyObjectRef,
        iter: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        if iter.get_attr("__next__").is_some() {
            Ok(iter)
        } else {
            Err(PyException::type_error(format!(
                "iter() returned non-iterator of type '{}'",
                owner.type_name()
            )))
        }
    }

    /// Resolve a dunder method on an Instance, skipping BuiltinBoundMethod
    /// (which comes from BuiltinType bases like list/dict and can't be called).
    /// Returns the method if it's a real callable (BoundMethod, Function, etc.).
    pub(crate) fn resolve_instance_dunder(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(method) = obj.get_attr(name) {
                    if matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                        return None;
                    }
                    return Some(method);
                }
            }
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                if let Some(class_val) = cd.namespace.read().get(name).cloned() {
                    return Some(Self::bind_class_val_for_instance(obj, inst, class_val));
                }
                for base in &cd.mro {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        if let Some(class_val) = bcd.namespace.read().get(name).cloned() {
                            return Some(Self::bind_class_val_for_instance(obj, inst, class_val));
                        }
                    }
                }
            }
            if let Some(method) = inst.attrs.read().get(name).cloned() {
                return Some(method);
            }
        }
        if let Some(method) = obj.get_attr(name) {
            if matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                return None;
            }
            return Some(method);
        }
        None
    }

    /// Bind a class-level attribute for instance access: wrap functions as BoundMethod,
    /// and leave descriptors (Instance with __get__) as-is for the VM to invoke __get__.
    fn bind_class_val_for_instance(
        obj: &PyObjectRef,
        inst: &InstanceData,
        class_val: PyObjectRef,
    ) -> PyObjectRef {
        match &class_val.payload {
            PyObjectPayload::Function(_)
            | PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure { .. } => PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: obj.clone(),
                    method: class_val,
                },
            }),
            PyObjectPayload::StaticMethod(func) => func.clone(),
            PyObjectPayload::ClassMethod(func) => PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: inst.class.clone(),
                    method: func.clone(),
                },
            }),
            _ => class_val,
        }
    }

    fn lookup_plain_class_dunder(inst: &InstanceData, name: &str) -> Option<PyObjectRef> {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            if let Some(class_val) = cd.namespace.read().get(name).cloned() {
                return Some(class_val);
            }
            for base in &cd.mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if let Some(class_val) = bcd.namespace.read().get(name).cloned() {
                        return Some(class_val);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn call_plain_instance_dunder(
        &mut self,
        obj: &PyObjectRef,
        inst: &InstanceData,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<Option<PyObjectRef>> {
        let Some(method) = Self::lookup_plain_class_dunder(inst, name) else {
            return Ok(None);
        };
        if ferrython_core::object::has_descriptor_get(&method) {
            let method = self.resolve_descriptor(&method, obj)?;
            return self.call_object(method, args).map(Some);
        }
        if !matches!(
            &method.payload,
            PyObjectPayload::Function(_)
                | PyObjectPayload::NativeFunction(_)
                | PyObjectPayload::NativeClosure(_)
        ) {
            return Ok(None);
        }
        if matches!(&method.payload, PyObjectPayload::Function(_)) {
            return match args.len() {
                0 => self
                    .call_object_one_arg_fast_or_fallback(method, obj.clone())
                    .map(Some),
                1 => self
                    .call_object_two_arg_fast_or_fallback(method, obj.clone(), args[0].clone())
                    .map(Some),
                _ => {
                    let mut call_args = Vec::with_capacity(args.len() + 1);
                    call_args.push(obj.clone());
                    call_args.extend(args);
                    self.call_object(method, call_args).map(Some)
                }
            };
        }
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(obj.clone());
        call_args.extend(args);
        self.call_object(method, call_args).map(Some)
    }

    /// Invoke __get__ on a descriptor to get the actual callable.
    /// Returns the original value if it's not a descriptor.
    pub(crate) fn resolve_descriptor(
        &mut self,
        val: &PyObjectRef,
        instance: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        if ferrython_core::object::is_property_like(val) {
            let Some(getter) = ferrython_core::object::property_field(val, "fget") else {
                return Err(PyException::attribute_error("unreadable attribute"));
            };
            if matches!(&getter.payload, PyObjectPayload::None) {
                return Err(PyException::attribute_error("unreadable attribute"));
            }
            let getter = crate::builtins::unwrap_abstract_fget(&getter);
            return self.call_object(getter, vec![instance.clone()]);
        }
        use ferrython_core::object::has_descriptor_get;
        if has_descriptor_get(val) {
            if let Some(get_method) = val.get_attr("__get__") {
                let owner = match &instance.payload {
                    PyObjectPayload::Instance(inst) => inst.class.clone(),
                    PyObjectPayload::Class(_) => instance.clone(),
                    _ => PyObject::none(),
                };
                let bound = if matches!(&get_method.payload, PyObjectPayload::BoundMethod { .. }) {
                    get_method
                } else {
                    PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: val.clone(),
                            method: get_method,
                        },
                    })
                };
                return self.call_object(bound, vec![instance.clone(), owner]);
            }
        }
        Ok(val.clone())
    }

    /// Get the __builtin_value__ from an Instance (for builtin type subclasses).
    pub(crate) fn get_builtin_value(obj: &PyObjectRef) -> Option<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            return inst.attrs.read().get("__builtin_value__").cloned();
        }
        None
    }

    /// Convert a Python object to a HashableKey, calling __hash__/__eq__ on instances.
    /// Dispatches are installed at VM init, so from_object will use them automatically.
    pub(crate) fn vm_to_hashable_key(&mut self, obj: &PyObjectRef) -> PyResult<HashableKey> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                if matches!(&value.payload, PyObjectPayload::FrozenSet(_)) {
                    return value.to_hashable_key();
                }
            }
        }
        obj.to_hashable_key()
    }

    fn try_inline_plain_attr_pair_eq(
        left_obj: &PyObjectRef,
        left_inst: &InstanceData,
        right_obj: &PyObjectRef,
    ) -> Option<bool> {
        if ferrython_stdlib::is_trace_active() || ferrython_stdlib::is_profile_active() {
            return None;
        }
        let eq_func = Self::lookup_plain_class_dunder(left_inst, "__eq__")?;
        let PyObjectPayload::Function(pyfunc) = &eq_func.payload else {
            return None;
        };
        let fields = attr_pair_eq_fields(pyfunc)?;
        let (first_left, first_right) =
            inline_attr_eq_by_name(left_obj, fields[0].0, right_obj, fields[0].1)?;
        if !inline_eq_bool(&first_left, &first_right)? {
            return Some(false);
        }
        let (second_left, second_right) =
            inline_attr_eq_by_name(left_obj, fields[1].0, right_obj, fields[1].1)?;
        Some(inline_eq_bool(&second_left, &second_right)?)
    }
}

#[inline(always)]
fn attr_pair_eq_fields(pyfunc: &PyFunction) -> Option<[(&str, &str); 2]> {
    let instrs = &pyfunc.code.instructions;
    if !pyfunc.is_simple
        || pyfunc.has_code_override()
        || pyfunc.code.arg_count != 2
        || instrs.len() != 18
        || instrs[0].op != Opcode::SetupExcept
        || instrs[4].op != Opcode::JumpIfFalseOrPop
        || instrs[4].arg != 8
        || instrs[8].op != Opcode::ReturnValue
        || instrs[9].op != Opcode::DupTop
        || instrs[10].op != Opcode::LoadGlobal
        || pyfunc
            .code
            .names
            .get(instrs[10].arg as usize)
            .map(|name| name.as_str())
            != Some("AttributeError")
        || instrs[11].op != Opcode::CompareOpPopJumpIfFalse
        || (instrs[11].arg >> 24) != 10
        || instrs[12].op != Opcode::PopTop
        || instrs[13].op != Opcode::PopTop
        || instrs[14].op != Opcode::PopTop
        || instrs[15].op != Opcode::LoadConstReturnValue
        || instrs[16].op != Opcode::EndFinally
        || instrs[17].op != Opcode::LoadConstReturnValue
    {
        return None;
    }
    let false_value = pyfunc.constant_cache.get(instrs[15].arg as usize)?;
    let trailing_value = pyfunc.constant_cache.get(instrs[17].arg as usize)?;
    if !matches!(&false_value.payload, PyObjectPayload::Bool(false))
        || !matches!(&trailing_value.payload, PyObjectPayload::None)
    {
        return None;
    }
    Some([
        attr_eq_field_names(pyfunc, instrs[1], instrs[2], instrs[3])?,
        attr_eq_field_names(pyfunc, instrs[5], instrs[6], instrs[7])?,
    ])
}

#[inline(always)]
fn attr_eq_field_names<'a>(
    pyfunc: &'a PyFunction,
    left_instr: Instruction,
    right_instr: Instruction,
    cmp_instr: Instruction,
) -> Option<(&'a str, &'a str)> {
    if cmp_instr.op != Opcode::CompareOp || cmp_instr.arg != 2 {
        return None;
    }
    let (left_local, left_name_idx) = unpack_local_name(left_instr)?;
    let (right_local, right_name_idx) = unpack_local_name(right_instr)?;
    if left_local != 0 || right_local != 1 {
        return None;
    }
    Some((
        pyfunc.code.names.get(left_name_idx)?.as_str(),
        pyfunc.code.names.get(right_name_idx)?.as_str(),
    ))
}

#[inline(always)]
fn unpack_local_name(instr: Instruction) -> Option<(usize, usize)> {
    if instr.op != Opcode::LoadFastLoadAttr {
        return None;
    }
    Some(((instr.arg >> 16) as usize, (instr.arg & 0xFFFF) as usize))
}

#[inline(always)]
fn inline_attr_eq_by_name(
    left_obj: &PyObjectRef,
    left_name: &str,
    right_obj: &PyObjectRef,
    right_name: &str,
) -> Option<(PyObjectRef, PyObjectRef)> {
    let left = inline_instance_attr_by_name(left_obj, left_name)?;
    let right = inline_instance_attr_by_name(right_obj, right_name)?;
    Some((left, right))
}

#[inline(always)]
fn inline_instance_attr_by_name(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    if inst.class_flags & (CLASS_FLAG_HAS_GETATTRIBUTE | CLASS_FLAG_HAS_GETATTR) != 0
        || inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0
        || inst.dict_storage.is_some()
    {
        return None;
    }
    let attrs = unsafe { &*inst.attrs.data_ptr() };
    if inst.is_special
        || attrs.contains_key("__deque__")
        || attrs.contains_key("__builtin_value__")
        || attrs.contains_key("__class__")
    {
        return None;
    }
    if let Some(value) = attrs.get(name) {
        return Some(value.clone());
    }
    let PyObjectPayload::Class(cd) = &inst.class.payload else {
        return None;
    };
    let vtable = unsafe { &*cd.method_vtable.data_ptr() };
    let class_value = vtable.get(name)?;
    match &class_value.payload {
        PyObjectPayload::Function(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure { .. }
        | PyObjectPayload::Property(_)
        | PyObjectPayload::ClassMethod(_)
        | PyObjectPayload::StaticMethod(_) => None,
        _ if has_descriptor_get(class_value) => None,
        _ => Some(class_value.clone()),
    }
}

#[inline(always)]
fn inline_eq_bool(left: &PyObjectRef, right: &PyObjectRef) -> Option<bool> {
    match (&left.payload, &right.payload) {
        (PyObjectPayload::Int(left), PyObjectPayload::Int(right)) => Some(left == right),
        (PyObjectPayload::Bool(left), PyObjectPayload::Bool(right)) => Some(left == right),
        (PyObjectPayload::Bool(left), PyObjectPayload::Int(PyInt::Small(right)))
        | (PyObjectPayload::Int(PyInt::Small(right)), PyObjectPayload::Bool(left)) => {
            Some((*left as i64) == *right)
        }
        (PyObjectPayload::Float(left), PyObjectPayload::Float(right)) => Some(left == right),
        (PyObjectPayload::Str(left), PyObjectPayload::Str(right)) => Some(left == right),
        (PyObjectPayload::None, PyObjectPayload::None) => Some(true),
        _ => None,
    }
}
