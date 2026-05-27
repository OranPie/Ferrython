//! VM-aware itertools helpers used by native stdlib call routing.

use crate::VirtualMachine;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use std::rc::Rc;

impl VirtualMachine {
    pub(crate) fn vm_itertools_groupby(
        &mut self,
        args: &[PyObjectRef],
        key_fn: Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("groupby requires iterable"));
        }
        let items = args[0].to_list()?;
        if items.is_empty() {
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List {
                    items: vec![],
                    index: 0,
                }),
            ))));
        }

        let mut result = Vec::new();
        let first_key = if let Some(ref kf) = key_fn {
            self.call_object(kf.clone(), vec![items[0].clone()])?
        } else {
            items[0].clone()
        };
        let mut current_key = first_key;
        let mut current_group = vec![items[0].clone()];

        for item in &items[1..] {
            let k = if let Some(ref kf) = key_fn {
                self.call_object(kf.clone(), vec![item.clone()])?
            } else {
                item.clone()
            };
            if k.py_to_string() == current_key.py_to_string() {
                current_group.push(item.clone());
            } else {
                let group_iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                    IteratorData::List {
                        items: current_group,
                        index: 0,
                    },
                ))));
                result.push(PyObject::tuple(vec![current_key, group_iter]));
                current_key = k;
                current_group = vec![item.clone()];
            }
        }
        let group_iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
            IteratorData::List {
                items: current_group,
                index: 0,
            },
        ))));
        result.push(PyObject::tuple(vec![current_key, group_iter]));
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: result,
                index: 0,
            }),
        ))))
    }

    pub(crate) fn vm_itertools_filterfalse(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
        let func = args[0].clone();
        let source = args[1].get_iter()?;
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::FilterFalse { func, source }),
        ))))
    }

    pub(crate) fn vm_itertools_starmap(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let func = args[0].clone();
        let source = args[1].get_iter()?;
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Starmap { func, source }),
        ))))
    }

    pub(crate) fn vm_itertools_accumulate(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
        let items = args[0].to_list()?;
        if items.is_empty() {
            return Ok(PyObject::list(vec![]));
        }
        let func = if args.len() >= 2
            && !matches!(
                &args[1].payload,
                PyObjectPayload::None | PyObjectPayload::Dict(_)
            ) {
            Some(args[1].clone())
        } else {
            None
        };
        let mut result = Vec::new();
        let mut acc = items[0].clone();
        result.push(acc.clone());
        for item in &items[1..] {
            acc = if let Some(ref f) = func {
                self.call_object(f.clone(), vec![acc, item.clone()])?
            } else {
                acc.add(item)?
            };
            result.push(acc.clone());
        }
        Ok(PyObject::list(result))
    }
}
