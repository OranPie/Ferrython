use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

fn iterator_setstate_index(arg: &PyObjectRef) -> PyResult<usize> {
    match &arg.payload {
        PyObjectPayload::Int(n) => {
            let value = n.to_i64().ok_or_else(|| {
                PyException::overflow_error("Python int too large to convert to C ssize_t")
            })?;
            Ok(value.max(0) as usize)
        }
        PyObjectPayload::Bool(value) => Ok(if *value { 1 } else { 0 }),
        _ => Err(PyException::type_error("an integer is required")),
    }
}

fn iterator_setstate_i64(arg: &PyObjectRef) -> PyResult<i64> {
    match &arg.payload {
        PyObjectPayload::Int(n) => {
            let value = n.to_i64().ok_or_else(|| {
                PyException::overflow_error("Python int too large to convert to C ssize_t")
            })?;
            Ok(value.max(0))
        }
        PyObjectPayload::Bool(value) => Ok(if *value { 1 } else { 0 }),
        _ => Err(PyException::type_error("an integer is required")),
    }
}

pub(super) fn set_iterator_state(
    iter: &PyObjectRef,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.len() != 1 {
        return Err(PyException::type_error(format!(
            "iterator.__setstate__() takes exactly one argument ({} given)",
            args.len()
        )));
    }
    match &iter.payload {
        PyObjectPayload::Iterator(iter_data) => {
            let mut data = iter_data.write();
            match &mut *data {
                IteratorData::List { items, index } => {
                    if *index <= items.len() {
                        *index = iterator_setstate_index(&args[0])?.min(items.len());
                    }
                    Ok(PyObject::none())
                }
                IteratorData::Tuple { items, index } => {
                    if *index <= items.len() {
                        *index = iterator_setstate_index(&args[0])?.min(items.len());
                    }
                    Ok(PyObject::none())
                }
                IteratorData::Str { chars, index } => {
                    if *index <= chars.len() {
                        *index = iterator_setstate_index(&args[0])?.min(chars.len());
                    }
                    Ok(PyObject::none())
                }
                IteratorData::SeqIter {
                    index, exhausted, ..
                } => {
                    if !*exhausted {
                        *index = iterator_setstate_i64(&args[0])?;
                    }
                    Ok(PyObject::none())
                }
                _ => Err(PyException::attribute_error(format!(
                    "'{}' object has no attribute '__setstate__'",
                    iter.type_name()
                ))),
            }
        }
        PyObjectPayload::RefIter { source, index } => {
            if index.get() == usize::MAX {
                return Ok(PyObject::none());
            }
            let total = match &source.payload {
                PyObjectPayload::List(cell) => unsafe { &*cell.data_ptr() }.len(),
                PyObjectPayload::Tuple(items) => items.len(),
                _ => {
                    return Err(PyException::attribute_error(format!(
                        "'{}' object has no attribute '__setstate__'",
                        iter.type_name()
                    )))
                }
            };
            if index.get() <= total {
                index.set(iterator_setstate_index(&args[0])?.min(total));
            }
            Ok(PyObject::none())
        }
        PyObjectPayload::RevRefIter { source, index } => {
            if index.get() == usize::MAX {
                return Ok(PyObject::none());
            }
            let total = match &source.payload {
                PyObjectPayload::List(cell) => unsafe { &*cell.data_ptr() }.len(),
                _ => {
                    return Err(PyException::attribute_error(format!(
                        "'{}' object has no attribute '__setstate__'",
                        iter.type_name()
                    )))
                }
            };
            if index.get() <= total {
                index.set(iterator_setstate_index(&args[0])?.min(total));
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '__setstate__'",
            iter.type_name()
        ))),
    }
}
