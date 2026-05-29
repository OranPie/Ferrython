use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    DequeIterData, IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    SyncUsize,
};
use std::rc::Rc;

use crate::builtins::deque_storage_len;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_iter_builtin(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if args.len() != 1 {
            return Ok(None);
        }
        let PyObjectPayload::Instance(inst) = &args[0].payload else {
            return Ok(None);
        };

        if inst.attrs.read().contains_key("__deque__") {
            return Ok(Some(PyObject::tracked(PyObjectPayload::DequeIter(
                Box::new(DequeIterData {
                    source: args[0].clone(),
                    index: SyncUsize::new(0),
                    expected_len: deque_storage_len(&args[0]).unwrap_or_default(),
                    reverse: false,
                }),
            ))));
        }
        if let Some(raw_iter) = Self::resolve_instance_dunder(&args[0], "__iter__") {
            let iter_method = self.resolve_descriptor(&raw_iter, &args[0])?;
            let result = self.call_object(iter_method, vec![])?;
            return Self::ensure_iterator_result(&args[0], result).map(Some);
        }
        if inst.dict_storage.is_some() {
            return args[0].get_iter().map(Some);
        }
        if let Some(builtin_value) = Self::get_builtin_value(&args[0]) {
            let iter = self.resolve_iterable(&builtin_value)?;
            return Ok(Some(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::HeldIter {
                    iter,
                    owner: Some(args[0].clone()),
                }),
            )))));
        }
        if args[0].get_attr("__getitem__").is_some() {
            return Ok(Some(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::SeqIter {
                    obj: args[0].clone(),
                    index: 0,
                    exhausted: false,
                }),
            )))));
        }
        Err(PyException::type_error(format!(
            "'{}' object is not iterable",
            args[0].type_name()
        )))
    }

    pub(super) fn call_next_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "next() requires at least 1 argument",
            ));
        }
        if let PyObjectPayload::Generator(gen_arc) = &args[0].payload {
            match self.resume_generator(gen_arc, PyObject::none()) {
                Ok(value) => return Ok(value),
                Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                    return Ok(args[1].clone());
                }
                Err(e) => return Err(e),
            }
        }
        match self.vm_iter_next(&args[0]) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => {
                if args.len() > 1 {
                    Ok(args[1].clone())
                } else {
                    Err(PyException::new(ExceptionKind::StopIteration, ""))
                }
            }
            Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                Ok(args[1].clone())
            }
            Err(e) => Err(e),
        }
    }
}
