use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::frame::ScopeKind;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_globals_kw(&self) -> PyObjectRef {
        if let Some(frame) = self.call_stack.last() {
            if let Some(globals_obj) = &frame.exec_globals {
                return globals_obj.clone();
            }
            let globals_arc = frame.globals.clone();
            return PyObject::wrap(PyObjectPayload::InstanceDict(globals_arc));
        }
        PyObject::dict(new_fx_hashkey_map())
    }

    pub(super) fn builtin_locals_kw(&self) -> PyObjectRef {
        if let Some(frame) = self.call_stack.last() {
            if let Some(locals) = &frame.exec_locals {
                return locals.clone();
            }
            if matches!(frame.scope_kind, ScopeKind::Module) {
                if let Some(globals_obj) = &frame.exec_globals {
                    return globals_obj.clone();
                }
            }
            if matches!(frame.scope_kind, ScopeKind::Class) {
                if let Some(local_names) = &frame.local_names {
                    return PyObject::wrap(PyObjectPayload::InstanceDict(local_names.clone()));
                }
            }
            return PyObject::dict(self.frame_locals_map(frame));
        }
        PyObject::dict(new_fx_hashkey_map())
    }

    pub(super) fn builtin_print_kw(
        &mut self,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let sep = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "sep")
            .map(|(_, v)| v.clone());
        let end = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "end")
            .map(|(_, v)| v.clone());
        let file_obj = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "file")
            .map(|(_, v)| v.clone());
        let flush = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "flush")
            .map(|(_, v)| v.is_truthy())
            .unwrap_or(false);
        self.vm_print(pos_args, sep, end, file_obj, flush)
    }
}
