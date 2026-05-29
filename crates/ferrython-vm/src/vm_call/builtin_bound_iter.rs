use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::vm_call::iterator_state::set_iterator_state;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_iterator_or_range_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if let PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::DequeIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. } = &bbm.receiver.payload
        {
            match bbm.method_name.as_str() {
                "__next__" => {
                    return Ok(Some(match self.vm_iter_next(&bbm.receiver)? {
                        Some(value) => value,
                        None => return Err(PyException::stop_iteration()),
                    }));
                }
                "__iter__" => {
                    return Ok(Some(bbm.receiver.clone()));
                }
                "__length_hint__" => {
                    let len = bbm.receiver.py_len().unwrap_or(0);
                    return Ok(Some(PyObject::int(len as i64)));
                }
                "__setstate__" => {
                    return set_iterator_state(&bbm.receiver, args).map(Some);
                }
                "__copy__" => {
                    if let Some(copy) = copy_reducible_iterator(&bbm.receiver)? {
                        return Ok(Some(copy));
                    }
                }
                "__deepcopy__" => {
                    if args.len() > 1 {
                        return Err(PyException::type_error(
                            "__deepcopy__() takes at most one argument",
                        ));
                    }
                    if let Some(copy) = copy_reducible_iterator(&bbm.receiver)? {
                        return Ok(Some(copy));
                    }
                }
                "__reduce__" | "__reduce_ex__" => {
                    if bbm.method_name.as_str() == "__reduce_ex__" && args.len() != 1 {
                        return Err(PyException::type_error(
                            "__reduce_ex__() takes exactly one argument",
                        ));
                    }
                    if let Some(reduced) = reduce_reducible_iterator(&bbm.receiver)? {
                        return Ok(Some(reduced));
                    }
                }
                _ => {}
            }
        }

        if let PyObjectPayload::Range(_rd) = &bbm.receiver.payload {
            match bbm.method_name.as_str() {
                "__contains__" | "count" | "index" => return Ok(None),
                _ => {}
            }
        }

        Ok(None)
    }
}

fn islice_state(receiver: &PyObjectRef) -> Option<(PyObjectRef, usize, usize, usize, usize)> {
    let PyObjectPayload::Iterator(iter_data) = &receiver.payload else {
        return None;
    };
    let data = iter_data.read();
    if let IteratorData::Islice {
        source,
        index,
        next_yield,
        stop,
        step,
    } = &*data
    {
        Some((source.clone(), *index, *next_yield, *stop, *step))
    } else {
        None
    }
}

fn copy_islice_iterator(receiver: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    Ok(
        islice_state(receiver).map(|(source, index, next_yield, stop, step)| {
            PyObject::wrap(PyObjectPayload::Iterator(std::rc::Rc::new(
                ferrython_core::object::PyCell::new(IteratorData::Islice {
                    source,
                    index,
                    next_yield,
                    stop,
                    step,
                }),
            )))
        }),
    )
}

fn copy_reducible_iterator(receiver: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    if let Some(copy) = copy_islice_iterator(receiver)? {
        return Ok(Some(copy));
    }
    let PyObjectPayload::Iterator(iter_data) = &receiver.payload else {
        return Ok(None);
    };
    let data = iter_data.read();
    let copy = match &*data {
        IteratorData::TakeWhile { func, source, done } => Some(IteratorData::TakeWhile {
            func: func.clone(),
            source: source.clone(),
            done: *done,
        }),
        IteratorData::DropWhile {
            func,
            source,
            dropping,
        } => Some(IteratorData::DropWhile {
            func: func.clone(),
            source: source.clone(),
            dropping: *dropping,
        }),
        _ => None,
    };
    Ok(copy.map(|data| {
        PyObject::wrap(PyObjectPayload::Iterator(std::rc::Rc::new(
            ferrython_core::object::PyCell::new(data),
        )))
    }))
}

fn reduce_islice_iterator(receiver: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    let Some((source, index, next_yield, stop, step)) = islice_state(receiver) else {
        return Ok(None);
    };
    let constructor = PyObject::native_closure("itertools.islice.__rebuild__", move |args| {
        if args.len() != 5 {
            return Err(PyException::type_error("invalid islice reduce state"));
        }
        let as_usize = |obj: &PyObjectRef, default: usize| -> usize {
            obj.as_int()
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(default)
        };
        Ok(PyObject::wrap(PyObjectPayload::Iterator(std::rc::Rc::new(
            ferrython_core::object::PyCell::new(IteratorData::Islice {
                source: args[0].clone(),
                index: as_usize(&args[1], 0),
                next_yield: as_usize(&args[2], 0),
                stop: as_usize(&args[3], usize::MAX),
                step: as_usize(&args[4], 1).max(1),
            }),
        ))))
    });
    let arg_obj = |value: usize| {
        i64::try_from(value)
            .map(PyObject::int)
            .unwrap_or_else(|_| PyObject::big_int(num_bigint::BigInt::from(value)))
    };
    Ok(Some(PyObject::tuple(vec![
        constructor,
        PyObject::tuple(vec![
            source,
            arg_obj(index),
            arg_obj(next_yield),
            arg_obj(stop),
            arg_obj(step),
        ]),
    ])))
}

fn reduce_reducible_iterator(receiver: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    if let Some(reduced) = reduce_islice_iterator(receiver)? {
        return Ok(Some(reduced));
    }
    let PyObjectPayload::Iterator(iter_data) = &receiver.payload else {
        return Ok(None);
    };
    let data = iter_data.read();
    match &*data {
        IteratorData::TakeWhile { func, source, done } => {
            let constructor =
                PyObject::native_closure("itertools.takewhile.__rebuild__", move |args| {
                    if args.len() != 3 {
                        return Err(PyException::type_error("invalid takewhile reduce state"));
                    }
                    Ok(PyObject::wrap(PyObjectPayload::Iterator(std::rc::Rc::new(
                        ferrython_core::object::PyCell::new(IteratorData::TakeWhile {
                            func: args[0].clone(),
                            source: args[1].clone(),
                            done: args[2].is_truthy(),
                        }),
                    ))))
                });
            Ok(Some(PyObject::tuple(vec![
                constructor,
                PyObject::tuple(vec![
                    func.clone(),
                    source.clone(),
                    PyObject::bool_val(*done),
                ]),
            ])))
        }
        IteratorData::DropWhile {
            func,
            source,
            dropping,
        } => {
            let constructor =
                PyObject::native_closure("itertools.dropwhile.__rebuild__", move |args| {
                    if args.len() != 3 {
                        return Err(PyException::type_error("invalid dropwhile reduce state"));
                    }
                    Ok(PyObject::wrap(PyObjectPayload::Iterator(std::rc::Rc::new(
                        ferrython_core::object::PyCell::new(IteratorData::DropWhile {
                            func: args[0].clone(),
                            source: args[1].clone(),
                            dropping: args[2].is_truthy(),
                        }),
                    ))))
                });
            Ok(Some(PyObject::tuple(vec![
                constructor,
                PyObject::tuple(vec![
                    func.clone(),
                    source.clone(),
                    PyObject::bool_val(*dropping),
                ]),
            ])))
        }
        _ => Ok(None),
    }
}
