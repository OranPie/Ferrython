//! VM object protocol helpers — dunder lookup, descriptors, hashing, and iterator validation.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    InstanceData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

impl VirtualMachine {
    /// Install thread-local __hash__ and __eq__ dispatch callbacks for HashableKey.
    /// Called once at VM creation so all set/dict operations can resolve custom hashing.
    pub(crate) fn install_hash_eq_dispatch(&mut self) {
        let vm_ptr = self as *mut VirtualMachine;
        ferrython_core::types::set_eq_dispatch(move |a: &PyObjectRef, b: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr };
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
                if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if matches!(&value.payload, PyObjectPayload::FrozenSet(_)) {
                        if let Ok(key) = value.to_hashable_key() {
                            use std::collections::hash_map::DefaultHasher;
                            use std::hash::{Hash, Hasher};
                            let mut hasher = DefaultHasher::new();
                            key.hash(&mut hasher);
                            return Some(hasher.finish() as i64);
                        }
                    }
                }
            }
            if let Some(hash_method) = obj.get_attr("__hash__") {
                if let Ok(result) = vm.call_object(hash_method, vec![]) {
                    return Some(result.as_int().unwrap_or(0));
                }
            }
            None
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

    /// Invoke __get__ on a descriptor to get the actual callable.
    /// Returns the original value if it's not a descriptor.
    pub(crate) fn resolve_descriptor(
        &mut self,
        val: &PyObjectRef,
        instance: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        use ferrython_core::object::has_descriptor_get;
        if has_descriptor_get(val) {
            if let Some(get_method) = val.get_attr("__get__") {
                let owner = if let PyObjectPayload::Instance(inst) = &instance.payload {
                    inst.class.clone()
                } else {
                    PyObject::none()
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
}
