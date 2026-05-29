use ferrython_core::error::PyResult;
use ferrython_core::object::{
    DequeIterData, IteratorData, PyCell, PyObject, PyObjectPayload, PyObjectRef, SyncUsize,
};
use std::rc::Rc;

use crate::builtins;
use crate::builtins::deque_storage_len;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_reversed_builtin(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if args.is_empty() {
            return Ok(None);
        }
        if matches!(
            &args[0].payload,
            PyObjectPayload::List(_) | PyObjectPayload::Range(_)
        ) {
            return builtins::dispatch("reversed", &[args[0].clone()]).map(Some);
        }
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if inst.attrs.read().contains_key("__deque__") {
                return Ok(Some(PyObject::tracked(PyObjectPayload::DequeIter(
                    Box::new(DequeIterData {
                        source: args[0].clone(),
                        index: SyncUsize::new(0),
                        expected_len: deque_storage_len(&args[0]).unwrap_or_default(),
                        reverse: true,
                    }),
                ))));
            }
            if let Some(rev_method) = Self::resolve_instance_dunder(&args[0], "__reversed__") {
                return self.call_object(rev_method, vec![]).map(Some);
            }
            if let Some(builtin_value) = Self::get_builtin_value(&args[0]) {
                let items = self.collect_iterable(&builtin_value)?;
                let iter = builtins::dispatch("reversed", &[PyObject::list(items)])?;
                return Ok(Some(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(args[0].clone()),
                    }),
                )))));
            }
        }
        let items = self.collect_iterable(&args[0])?;
        builtins::dispatch("reversed", &[PyObject::list(items)]).map(Some)
    }
}
