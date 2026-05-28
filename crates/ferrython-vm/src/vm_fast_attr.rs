//! Fast attribute and method load helpers for the VM dispatch loop.

use crate::frame::{AttrInlineCache, Frame};
use crate::vm_fast_paths::native_function_binds_to_class;
use crate::vm_method_cache::cached_method_name;
use compact_str::CompactString;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{
    has_descriptor_get, lookup_in_class_mro, PyObject, PyObjectPayload, PyObjectRef,
    CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_GETATTRIBUTE, CLASS_FLAG_HAS_SETATTR,
    CLASS_FLAG_HAS_SLOTS,
};

pub(crate) enum FastAttrResult {
    Handled,
    Fallback,
    UnboundLocal(usize),
}

#[inline(always)]
pub(crate) fn try_fast_attr(frame: &mut Frame, instr: Instruction) -> FastAttrResult {
    match instr.op {
        Opcode::LoadFastLoadAttr => {
            let local_idx = (instr.arg >> 16) as usize;
            let name_idx = (instr.arg & 0xFFFF) as usize;
            try_load_fast_attr(frame, local_idx, name_idx)
        }
        Opcode::LoadFastLoadAttrStoreFast => {
            let local_idx = ((instr.arg >> 20) & 0x3FF) as usize;
            let name_idx = ((instr.arg >> 10) & 0x3FF) as usize;
            let store_idx = (instr.arg & 0x3FF) as usize;
            try_load_fast_attr_store(frame, local_idx, name_idx, store_idx)
        }
        Opcode::LoadFastLoadMethod => {
            let local_idx = (instr.arg >> 16) as usize;
            let name_idx = (instr.arg & 0xFFFF) as usize;
            try_load_fast_method(frame, local_idx, name_idx)
        }
        Opcode::LoadAttr => try_load_attr(frame, instr.arg as usize),
        Opcode::LoadMethod => try_load_method(frame, instr.arg as usize),
        Opcode::StoreAttr => try_store_attr(frame, instr.arg as usize),
        _ => FastAttrResult::Fallback,
    }
}

#[inline(always)]
fn local_ref(frame: &Frame, idx: usize) -> Option<&PyObjectRef> {
    unsafe { frame.locals.get_unchecked(idx).as_ref() }
}

#[inline(always)]
fn stack_ref(frame: &Frame, idx: usize) -> &PyObjectRef {
    unsafe { frame.stack.get_unchecked(idx) }
}

#[inline(always)]
fn push(frame: &mut Frame, value: PyObjectRef) {
    unsafe { frame.push_unchecked(value) };
}

#[inline(always)]
fn set_local(frame: &mut Frame, idx: usize, value: PyObjectRef) {
    unsafe { *frame.locals.get_unchecked_mut(idx) = Some(value) };
}

#[inline(always)]
fn replace_stack_top(frame: &mut Frame, value: PyObjectRef) {
    let len = frame.stack.len();
    unsafe { *frame.stack.get_unchecked_mut(len - 1) = value };
}

#[inline(always)]
fn pop(frame: &mut Frame) -> PyObjectRef {
    frame.stack.pop().expect("stack underflow")
}

#[inline(always)]
fn try_store_attr(frame: &mut Frame, name_idx: usize) -> FastAttrResult {
    let name = frame.code.names[name_idx].clone();
    let stack_len = frame.stack.len();
    let fast = if stack_len >= 2 {
        if let PyObjectPayload::Instance(inst) = &stack_ref(frame, stack_len - 1).payload {
            inst.class_flags
                & (CLASS_FLAG_HAS_SETATTR | CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_SLOTS)
                == 0
                && !(name.as_str() == "__callback__"
                    && inst.attrs.read().contains_key("__weakref_ref__"))
                && !inst.attrs.read().contains_key("__weakref_target__")
                && !inst.attrs.read().contains_key("__deque__")
        } else {
            false
        }
    } else {
        false
    };
    if !fast {
        return FastAttrResult::Fallback;
    }

    let obj = pop(frame);
    let value = pop(frame);
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let map = unsafe { &mut *inst.attrs.data_ptr() };
        if let Some(slot) = map.get_mut(&name) {
            *slot = value;
        } else {
            map.insert(name, value);
        }
    }
    FastAttrResult::Handled
}

#[inline(always)]
fn try_load_fast_attr(frame: &mut Frame, local_idx: usize, name_idx: usize) -> FastAttrResult {
    let Some(obj) = local_ref(frame, local_idx) else {
        return FastAttrResult::UnboundLocal(local_idx);
    };
    let obj = obj.clone();
    if let Some(value) = fast_instance_attr_value(frame, &obj, name_idx, false, false) {
        push(frame, value);
        FastAttrResult::Handled
    } else {
        push(frame, obj);
        FastAttrResult::Fallback
    }
}

#[inline(always)]
fn try_load_fast_attr_store(
    frame: &mut Frame,
    local_idx: usize,
    name_idx: usize,
    store_idx: usize,
) -> FastAttrResult {
    let Some(obj) = local_ref(frame, local_idx) else {
        return FastAttrResult::UnboundLocal(local_idx);
    };
    let obj = obj.clone();
    if let Some(value) = fast_instance_attr_value(frame, &obj, name_idx, true, false) {
        set_local(frame, store_idx, value);
        FastAttrResult::Handled
    } else {
        push(frame, obj);
        FastAttrResult::Fallback
    }
}

#[inline(always)]
fn try_load_attr(frame: &mut Frame, name_idx: usize) -> FastAttrResult {
    let obj = unsafe { frame.stack.get_unchecked(frame.stack.len() - 1) }.clone();
    if let Some(value) = fast_instance_attr_value(frame, &obj, name_idx, false, true) {
        replace_stack_top(frame, value);
        FastAttrResult::Handled
    } else {
        FastAttrResult::Fallback
    }
}

#[inline(always)]
fn try_load_fast_method(frame: &mut Frame, local_idx: usize, name_idx: usize) -> FastAttrResult {
    let Some(obj) = local_ref(frame, local_idx) else {
        return FastAttrResult::UnboundLocal(local_idx);
    };
    let obj = obj.clone();
    let name = frame.code.names[name_idx].clone();
    let mut fast_kind = 0u8;
    let mut fast_value: Option<PyObjectRef> = None;
    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            let skip_getattribute = inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0;
            if skip_getattribute
                && inst.dict_storage.is_none()
                && !inst.is_special
                && !inst.attrs.read().contains_key("__deque__")
                && name.as_str() != "__class__"
                && name.as_str() != "__dict__"
            {
                let class = &inst.class;
                if let PyObjectPayload::Class(cd) = &class.payload {
                    let ip = frame.ip as u32;
                    if let Some(cached) = attr_cache_lookup(frame, ip, cd.class_version) {
                        match &cached.payload {
                            PyObjectPayload::Function(_) => {
                                fast_kind = 1;
                                fast_value = Some(cached);
                            }
                            PyObjectPayload::NativeFunction(nf) => {
                                fast_kind = method_bind_kind(cd, &name, nf.name.as_str());
                                fast_value = Some(cached);
                            }
                            _ => {}
                        }
                    }
                    if fast_kind == 0 {
                        let method_hit = class_method_hit(class, cd, name.as_str());
                        if let Some(class_value) = method_hit {
                            if matches!(&class_value.payload, PyObjectPayload::Function(_)) {
                                fast_kind = 1;
                                attr_cache_insert(frame, ip, cd.class_version, class_value.clone());
                                fast_value = Some(class_value);
                            } else if let PyObjectPayload::NativeFunction(nf) = &class_value.payload
                            {
                                fast_kind = method_bind_kind(cd, &name, nf.name.as_str());
                                attr_cache_insert(frame, ip, cd.class_version, class_value.clone());
                                fast_value = Some(class_value);
                            }
                        } else if let Some(value) = unsafe { &*inst.attrs.data_ptr() }
                            .get(name.as_str())
                            .cloned()
                        {
                            fast_kind = 2;
                            fast_value = Some(value);
                        }
                    }
                }
            }
        }
        PyObjectPayload::List(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Tuple(_)
        | PyObjectPayload::Set(_)
        | PyObjectPayload::ByteArray(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::InstanceDict(_) => {
            if name.as_str() != "__class__" {
                fast_kind = 3;
            }
        }
        _ => {}
    }
    match fast_kind {
        1 => {
            push(frame, fast_value.expect("method fast value"));
            push(frame, obj);
            FastAttrResult::Handled
        }
        2 => {
            push(frame, PyObject::none());
            push(frame, fast_value.expect("callable fast value"));
            FastAttrResult::Handled
        }
        3 => {
            let name_obj =
                cached_method_name(name.as_str()).unwrap_or_else(|| PyObject::str_val(name));
            push(frame, name_obj);
            push(frame, obj);
            FastAttrResult::Handled
        }
        _ => {
            push(frame, obj);
            FastAttrResult::Fallback
        }
    }
}

#[inline(always)]
fn try_load_method(frame: &mut Frame, name_idx: usize) -> FastAttrResult {
    if frame.stack.is_empty() {
        return FastAttrResult::Fallback;
    }

    let obj = unsafe { frame.stack.get_unchecked(frame.stack.len() - 1) }.clone();
    let name = frame.code.names[name_idx].clone();
    let mut fast_kind = 0u8;
    let mut fast_value: Option<PyObjectRef> = None;

    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            let skip_getattribute = inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0;
            if skip_getattribute
                && inst.dict_storage.is_none()
                && !inst.is_special
                && !inst.attrs.read().contains_key("__deque__")
                && name.as_str() != "__class__"
                && name.as_str() != "__dict__"
            {
                let class = &inst.class;
                if let PyObjectPayload::Class(cd) = &class.payload {
                    let ip = frame.ip as u32;
                    if let Some(cached) = attr_cache_lookup(frame, ip, cd.class_version) {
                        match &cached.payload {
                            PyObjectPayload::Function(_) => {
                                fast_kind = 1;
                                fast_value = Some(cached);
                            }
                            PyObjectPayload::NativeFunction(nf) => {
                                fast_kind = method_bind_kind(cd, &name, nf.name.as_str());
                                fast_value = Some(cached);
                            }
                            _ => {}
                        }
                    }

                    if fast_kind == 0 {
                        if let Some(class_value) = class_method_hit(class, cd, name.as_str()) {
                            match &class_value.payload {
                                PyObjectPayload::Function(_) => {
                                    fast_kind = 1;
                                    attr_cache_insert(
                                        frame,
                                        ip,
                                        cd.class_version,
                                        class_value.clone(),
                                    );
                                    fast_value = Some(class_value);
                                }
                                PyObjectPayload::NativeFunction(nf) => {
                                    fast_kind = method_bind_kind(cd, &name, nf.name.as_str());
                                    attr_cache_insert(
                                        frame,
                                        ip,
                                        cd.class_version,
                                        class_value.clone(),
                                    );
                                    fast_value = Some(class_value);
                                }
                                _ => {}
                            }
                        } else if let Some(value) = unsafe { &*inst.attrs.data_ptr() }
                            .get(name.as_str())
                            .cloned()
                        {
                            fast_kind = 2;
                            fast_value = Some(value);
                        }
                    }
                }
            }
        }
        PyObjectPayload::List(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Tuple(_)
        | PyObjectPayload::Set(_)
        | PyObjectPayload::ByteArray(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::InstanceDict(_) => {
            if name.as_str() != "__class__" {
                fast_kind = 3;
            }
        }
        _ => {}
    }

    match fast_kind {
        1 => {
            let method = fast_value.expect("method fast value");
            let receiver = frame.stack.pop().expect("receiver");
            push(frame, method);
            push(frame, receiver);
            FastAttrResult::Handled
        }
        2 => {
            replace_stack_top(frame, PyObject::none());
            push(frame, fast_value.expect("callable fast value"));
            FastAttrResult::Handled
        }
        3 => {
            let name_obj =
                cached_method_name(name.as_str()).unwrap_or_else(|| PyObject::str_val(name));
            let receiver_idx = frame.stack.len() - 1;
            push(frame, name_obj);
            frame.stack.swap(receiver_idx, receiver_idx + 1);
            FastAttrResult::Handled
        }
        _ => FastAttrResult::Fallback,
    }
}

#[inline(always)]
fn fast_instance_attr_value(
    frame: &mut Frame,
    obj: &PyObjectRef,
    name_idx: usize,
    use_cache: bool,
    generic_load_attr: bool,
) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    if inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE != 0
        || inst.attrs.read().contains_key("__deque__")
    {
        return None;
    }
    let name = &frame.code.names[name_idx];
    if let PyObjectPayload::Class(cd) = &inst.class.payload {
        if use_cache {
            let ip = frame.ip as u32;
            if let Some(cached) = attr_cache_lookup(frame, ip, cd.class_version) {
                return Some(cached);
            }
        }
        if !generic_load_attr && name.as_str() == "__class__" {
            return Some(inst.class.clone());
        }
        let attrs = unsafe { &*inst.attrs.data_ptr() };
        if let Some(value) = attrs.get(name.as_str()) {
            if attr_value_can_load_from_instance(value, inst.class_flags, generic_load_attr) {
                return Some(value.clone());
            }
            return None;
        }
        let _ = attrs;
        if generic_load_attr && name.as_str() == "__class__" {
            return Some(inst.class.clone());
        }
        if inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0 {
            return None;
        }
        let value = fast_class_attr_value(cd, name.as_str(), generic_load_attr)?;
        if use_cache {
            attr_cache_insert(frame, frame.ip as u32, cd.class_version, value.clone());
        }
        Some(value)
    } else {
        None
    }
}

#[inline(always)]
fn attr_value_can_load_from_instance(
    value: &PyObjectRef,
    class_flags: u8,
    generic_load_attr: bool,
) -> bool {
    match &value.payload {
        PyObjectPayload::Function(_) | PyObjectPayload::Property(_) => false,
        _ if generic_load_attr && class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0 => false,
        _ => true,
    }
}

#[inline(always)]
fn fast_class_attr_value(
    cd: &ferrython_core::object::ClassData,
    name: &str,
    generic_load_attr: bool,
) -> Option<PyObjectRef> {
    let vtable = unsafe { &*cd.method_vtable.data_ptr() };
    if vtable.is_empty() {
        return None;
    }
    let class_value = vtable.get(name)?;
    match &class_value.payload {
        PyObjectPayload::Function(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure { .. }
        | PyObjectPayload::Property(_)
        | PyObjectPayload::ClassMethod(_)
        | PyObjectPayload::StaticMethod(_) => None,
        PyObjectPayload::Instance(cp_inst)
            if cp_inst
                .attrs
                .read()
                .contains_key("__cached_property_func__") =>
        {
            None
        }
        _ if generic_load_attr && has_descriptor_get(class_value) => None,
        _ => Some(class_value.clone()),
    }
}

#[inline(always)]
fn class_method_hit(
    class: &PyObjectRef,
    cd: &ferrython_core::object::ClassData,
    name: &str,
) -> Option<PyObjectRef> {
    let vtable = unsafe { &*cd.method_vtable.data_ptr() };
    if !vtable.is_empty() {
        vtable.get(name).cloned()
    } else {
        let namespace = unsafe { &*cd.namespace.data_ptr() };
        namespace
            .get(name)
            .cloned()
            .or_else(|| lookup_in_class_mro(class, name))
    }
}

#[inline(always)]
fn method_bind_kind(
    cd: &ferrython_core::object::ClassData,
    attr_name: &CompactString,
    native_name: &str,
) -> u8 {
    if native_function_binds_to_class(cd, attr_name, native_name) {
        1
    } else {
        2
    }
}

#[inline(always)]
fn attr_cache_lookup(frame: &Frame, ip: u32, class_version: u64) -> Option<PyObjectRef> {
    frame
        .attr_ic
        .as_ref()
        .and_then(|cache| cache.lookup(ip, class_version))
        .cloned()
}

#[inline(always)]
fn attr_cache_insert(frame: &mut Frame, ip: u32, class_version: u64, value: PyObjectRef) {
    frame
        .attr_ic
        .get_or_insert_with(|| Box::new(AttrInlineCache::empty()))
        .insert(ip, class_version, value);
}
