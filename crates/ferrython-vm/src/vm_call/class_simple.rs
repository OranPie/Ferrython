use compact_str::CompactString;
use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::frame::{Frame, ScopeKind};
use crate::vm_call::class_inline::analyze_trivial_init;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn try_instantiate_simple_class(
        &mut self,
        cls: &PyObjectRef,
        pos_args: &mut Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        // ── FAST PATH: simple class — skip ABC check entirely ──
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if !cd.is_simple_class.get() || class_inherits_builtin_type(cd, "type") {
                return Ok(None);
            }
            if cd.is_simple_class.get()
                && kwargs.is_empty()
                && !cd.has_custom_new.get()
                && ferrython_core::object::lookup_in_class_mro(cls, "__new__")
                    .map(|new| match &new.payload {
                        PyObjectPayload::NativeFunction(nf) => nf.name.as_str() == "__new__",
                        PyObjectPayload::BuiltinBoundMethod(bbm) => {
                            bbm.method_name.as_str() == "__new__"
                                && matches!(
                                    &bbm.receiver.payload,
                                    PyObjectPayload::BuiltinType(name) if name.as_str() == "object"
                                )
                        }
                        _ => false,
                    })
                    .unwrap_or(true)
                && !cd.is_dict_subclass
                && !cd.has_descriptors
                && cd.builtin_base_name.is_none()
            {
                let instance = PyObject::instance(cls.clone());
                // Use cached __init__ — unsafe data_ptr avoids RefCell borrow overhead
                let init_fn = {
                    let cached_ptr = unsafe { &*cd.cached_init.data_ptr() };
                    if cached_ptr.is_some() {
                        cached_ptr.clone()
                    } else {
                        let found = cd
                            .method_vtable
                            .read()
                            .get("__init__")
                            .cloned()
                            .or_else(|| cd.namespace.read().get("__init__").cloned())
                            .or_else(|| {
                                ferrython_core::object::lookup_in_class_mro(cls, "__init__")
                            });
                        *cd.cached_init.write() = found.clone();
                        found
                    }
                };
                if let Some(init_fn) = init_fn {
                    seed_exception_args(cd.is_exception_subclass, &instance, pos_args);
                    // Fast path: simple Python function __init__ — inline frame creation
                    let total_args = pos_args.len() + 1; // +1 for self
                    let is_simple_init = if let PyObjectPayload::Function(pf) = &init_fn.payload {
                        pf.is_simple
                            && pf.code.arg_count as usize == total_args
                            && pf.closure.is_empty()
                    } else {
                        false
                    };
                    if is_simple_init {
                        // Check if __init__ is trivially inlinable (only LOAD_FAST+STORE_ATTR pairs)
                        // Use unsafe data_ptr to avoid RefCell borrow + Vec clone on the hot path
                        let cached_ptr = unsafe { &*cd.cached_init_inline.data_ptr() };
                        let inline_slots: &Option<Vec<(usize, usize)>> = match cached_ptr {
                            Some(ref info) => info,
                            None => {
                                let info = analyze_trivial_init(unsafe {
                                    match &init_fn.payload {
                                        PyObjectPayload::Function(pf) => &pf.code,
                                        _ => std::hint::unreachable_unchecked(),
                                    }
                                });
                                *cd.cached_init_inline.write() = Some(info);
                                unsafe { (&*cd.cached_init_inline.data_ptr()).as_ref().unwrap() }
                            }
                        };
                        if let Some(ref slots) = inline_slots {
                            // INLINE: directly set attrs on instance — no frame needed
                            if let PyObjectPayload::Instance(inst) = &instance.payload {
                                let code: &CodeObject = unsafe {
                                    match &init_fn.payload {
                                        PyObjectPayload::Function(pf) => &pf.code,
                                        _ => std::hint::unreachable_unchecked(),
                                    }
                                };
                                let map = unsafe { &mut *inst.attrs.data_ptr() };
                                for &(arg_idx, name_idx) in slots.iter() {
                                    // arg_idx is 1-based (0=self); pos_args is 0-based
                                    let value = std::mem::replace(
                                        &mut pos_args[arg_idx - 1],
                                        PyObject::none(),
                                    );
                                    map.insert(code.names[name_idx].clone(), value);
                                }
                            }
                        } else {
                            // Not inlinable — use frame
                            let mut new_frame = unsafe {
                                let pf_ptr = match &init_fn.payload {
                                    PyObjectPayload::Function(pf) => {
                                        &**pf as *const ferrython_core::types::PyFunction
                                    }
                                    _ => std::hint::unreachable_unchecked(),
                                };
                                Frame::new_borrowed(
                                    &*pf_ptr,
                                    init_fn,
                                    &self.builtins,
                                    &mut self.frame_pool,
                                )
                            };
                            // locals[0] = self, locals[1..] = pos_args
                            new_frame.locals[0] = Some(instance.clone());
                            for (i, arg) in std::mem::take(pos_args).into_iter().enumerate() {
                                new_frame.locals[i + 1] = Some(arg);
                            }
                            new_frame.scope_kind = ScopeKind::Function;
                            new_frame.discard_return = false;
                            self.call_stack.push(new_frame);
                            let init_result = self.run_frame();
                            if let Some(f) = self.call_stack.pop() {
                                f.recycle(&mut self.frame_pool);
                            }
                            let init_result = init_result?;
                            if !matches!(&init_result.payload, PyObjectPayload::None) {
                                return Err(PyException::type_error(
                                    "__init__() should return None, not '".to_string()
                                        + init_result.type_name()
                                        + "'",
                                ));
                            }
                        }
                    } else {
                        pos_args.insert(0, instance.clone());
                        let init_result = self.call_object(init_fn, std::mem::take(pos_args))?;
                        if !matches!(&init_result.payload, PyObjectPayload::None) {
                            return Err(PyException::type_error(
                                "__init__() should return None, not '".to_string()
                                    + init_result.type_name()
                                    + "'",
                            ));
                        }
                    }
                } else if cd.is_exception_subclass {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        if !attrs.contains_key("args") {
                            if pos_args.len() == 1 {
                                attrs.insert(CompactString::from("message"), pos_args[0].clone());
                            }
                            attrs.insert(
                                CompactString::from("args"),
                                PyObject::tuple(std::mem::take(pos_args)),
                            );
                        }
                    }
                } else if !pos_args.is_empty() {
                    return Err(PyException::type_error(format!(
                        "{}() takes no arguments",
                        cd.name
                    )));
                }
                if cd.is_exception_subclass {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        if !attrs.contains_key("args") {
                            attrs.insert(CompactString::from("args"), PyObject::tuple(vec![]));
                        }
                    }
                }
                return Ok(Some(instance));
            }
        }
        Ok(None)
    }
}

fn class_inherits_builtin_type(cd: &ferrython_core::object::ClassData, type_name: &str) -> bool {
    cd.bases
        .iter()
        .chain(cd.mro.iter())
        .any(|base| match &base.payload {
            PyObjectPayload::BuiltinType(name) => name.as_str() == type_name,
            PyObjectPayload::Class(base_cd) => {
                base_cd.name.as_str() == type_name
                    || class_inherits_builtin_type(base_cd, type_name)
            }
            _ => false,
        })
}

fn seed_exception_args(is_exception_subclass: bool, instance: &PyObjectRef, args: &[PyObjectRef]) {
    if !is_exception_subclass {
        return;
    }
    let PyObjectPayload::Instance(inst) = &instance.payload else {
        return;
    };
    let mut attrs = inst.attrs.write();
    if args.len() == 1 {
        attrs.insert(CompactString::from("message"), args[0].clone());
    }
    attrs.insert(CompactString::from("args"), PyObject::tuple(args.to_vec()));
}
