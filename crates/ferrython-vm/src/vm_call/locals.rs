use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn frame_locals_map(&self, frame: &Frame) -> IndexMap<HashableKey, PyObjectRef> {
        let mut map = IndexMap::new();
        for (i, name) in frame.code.varnames.iter().enumerate() {
            if let Some(Some(val)) = frame.locals.get(i) {
                map.insert(HashableKey::str_key(name.clone()), val.clone());
            }
        }
        for (k, v) in frame.local_names_snapshot() {
            map.insert(HashableKey::str_key(k.clone()), v.clone());
        }
        if !matches!(frame.scope_kind, ScopeKind::Class) {
            for (i, name) in frame
                .code
                .cellvars
                .iter()
                .chain(frame.code.freevars.iter())
                .enumerate()
            {
                if let Some(cell) = frame.cells.get(i) {
                    let cell_val = cell.read();
                    if let Some(val) = cell_val.as_ref() {
                        map.insert(HashableKey::str_key(name.clone()), val.clone());
                    }
                }
            }
        }
        map
    }

    /// Collect the current frame's local variables into a dict.
    /// At module scope, locals() == globals().
    pub(super) fn collect_locals_dict(&self) -> PyResult<PyObjectRef> {
        let frame = self.call_stack.last().unwrap();
        if let Some(locals) = &frame.exec_locals {
            return Ok(locals.clone());
        }
        if matches!(frame.scope_kind, ScopeKind::Module) {
            // At module level, locals() == globals()
            let g = frame.globals.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = g
                .iter()
                .map(|(k, v)| {
                    (
                        PyObject::str_val(CompactString::from(k.as_str())),
                        v.clone(),
                    )
                })
                .collect();
            drop(g);
            return Ok(PyObject::dict_from_pairs(pairs));
        }
        if matches!(frame.scope_kind, ScopeKind::Class) {
            if let Some(local_names) = &frame.local_names {
                return Ok(PyObject::wrap(
                    ferrython_core::object::PyObjectPayload::InstanceDict(local_names.clone()),
                ));
            }
        }
        Ok(PyObject::dict(self.frame_locals_map(frame)))
    }
}
