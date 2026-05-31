use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    DequeIterData, IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    SyncUsize,
};
use std::rc::Rc;

use crate::builtins;
use crate::builtins::deque_storage_len;
use crate::VirtualMachine;

impl VirtualMachine {
    fn instance_sequence_reversed_fallback(obj: &PyObjectRef) -> bool {
        let PyObjectPayload::Instance(inst) = &obj.payload else {
            return false;
        };
        if inst.attrs.read().contains_key("__deque__") || inst.dict_storage.is_some() {
            return false;
        }
        let has_getitem = Self::resolve_instance_dunder(obj, "__getitem__").is_some();
        let has_len = Self::resolve_instance_dunder(obj, "__len__").is_some();
        let has_contains = Self::resolve_instance_dunder(obj, "__contains__").is_some();
        has_getitem && has_len && has_contains
    }

    fn collect_reversed_sequence_fallback(
        &mut self,
        obj: &PyObjectRef,
    ) -> PyResult<Option<PyObjectRef>> {
        if !Self::instance_sequence_reversed_fallback(obj) {
            return Ok(None);
        }
        let len_obj = self.call_object(
            Self::resolve_instance_dunder(obj, "__len__").unwrap(),
            vec![],
        )?;
        let len = len_obj.to_int().map_err(|_| {
            PyException::type_error(format!(
                "'{}' object cannot be interpreted as an integer",
                len_obj.type_name()
            ))
        })?;
        if len < 0 {
            return Err(PyException::value_error("__len__() should return >= 0"));
        }
        let Some(getitem) = Self::resolve_instance_dunder(obj, "__getitem__") else {
            return Ok(None);
        };
        let mut items = Vec::new();
        for index in (0..len).rev() {
            items.push(self.call_object(getitem.clone(), vec![PyObject::int(index)])?);
        }
        Ok(Some(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List { items, index: 0 }),
        )))))
    }

    fn instance_blocks_reversed(obj: &PyObjectRef) -> bool {
        let PyObjectPayload::Instance(inst) = &obj.payload else {
            return false;
        };
        let PyObjectPayload::Class(cd) = &inst.class.payload else {
            return false;
        };
        cd.mro.iter().any(|base| {
            if let PyObjectPayload::Class(base_cd) = &base.payload {
                let ns = base_cd.namespace.read();
                ns.get("__reversed__")
                    .map(|value| matches!(&value.payload, PyObjectPayload::None))
                    .unwrap_or(false)
            } else {
                false
            }
        })
    }

    pub(super) fn call_reversed_builtin(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if args.is_empty() {
            return Ok(None);
        }
        if args.len() != 1 {
            return builtins::dispatch("reversed", args).map(Some);
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
            if Self::instance_blocks_reversed(&args[0]) {
                return Err(PyException::type_error(format!(
                    "'{}' object is not reversible",
                    args[0].type_name()
                )));
            }
            if let Some(iter) = self.collect_reversed_sequence_fallback(&args[0])? {
                return Ok(Some(iter));
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
