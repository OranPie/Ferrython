use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

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
            let mut map = IndexMap::new();
            for (i, name) in frame.code.varnames.iter().enumerate() {
                if let Some(Some(val)) = frame.locals.get(i) {
                    map.insert(HashableKey::str_key(name.clone()), val.clone());
                }
            }
            if frame.code.varnames.is_empty() {
                let g = frame.globals.read();
                for (k, v) in g.iter() {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                drop(g);
                for (k, v) in frame.local_names_iter() {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
            }
            return PyObject::dict(map);
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
