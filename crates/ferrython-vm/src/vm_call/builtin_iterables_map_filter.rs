use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{IteratorData, PyCell, PyObject, PyObjectPayload, PyObjectRef};
use std::rc::Rc;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_map_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "map() requires at least 2 arguments",
            ));
        }
        let func_obj = args[0].clone();
        let mut sources = Vec::with_capacity(args.len() - 1);
        for arg in &args[1..] {
            sources.push(self.resolve_iterable(arg)?);
        }
        if sources.len() == 1 {
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::MapOne {
                    func: func_obj,
                    source: sources.pop().unwrap(),
                }),
            ))));
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Map {
                func: func_obj,
                sources,
            }),
        ))))
    }

    pub(super) fn call_filter_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "filter() requires at least 2 arguments",
            ));
        }
        let func_obj = args[0].clone();
        let source = self.resolve_iterable(&args[1])?;
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Filter {
                func: func_obj,
                source,
            }),
        ))))
    }
}
