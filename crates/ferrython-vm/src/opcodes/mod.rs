//! Opcode group handlers for the VM.
//!
//! This module splits the monolithic `execute_one` match into logically
//! grouped methods, each handling a family of related opcodes.

mod arithmetic;
mod compare;
mod data;
mod exception;
mod flow;
mod misc;

use crate::frame::Frame;
use crate::VirtualMachine;
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

/// Unwrap IntEnum/IntFlag members to their `_value_` for arithmetic operations.
pub(super) fn unwrap_int_enum(obj: &PyObjectRef) -> PyObjectRef {
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
