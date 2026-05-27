use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectRef};

use crate::frame::ScopeKind;
use crate::VirtualMachine;

impl VirtualMachine {
    /// Collect the current frame's local variables into a dict.
    /// At module scope, locals() == globals().
    pub(super) fn collect_locals_dict(&self) -> PyResult<PyObjectRef> {
        let frame = self.call_stack.last().unwrap();
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
        let mut pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
        // Fast locals (function parameters and local variables)
        for (i, name) in frame.code.varnames.iter().enumerate() {
            if let Some(Some(val)) = frame.locals.get(i) {
                pairs.push((PyObject::str_val(name.clone()), val.clone()));
            }
        }
        // local_names (class scope, etc.)
        for (k, v) in frame.local_names_iter() {
            pairs.push((PyObject::str_val(k.clone()), v.clone()));
        }
        // Cell and free variables
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
                    pairs.push((PyObject::str_val(name.clone()), val.clone()));
                }
            }
        }
        Ok(PyObject::dict_from_pairs(pairs))
    }
}
