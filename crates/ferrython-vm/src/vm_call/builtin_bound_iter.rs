use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    BuiltinBoundMethodData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
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
                _ => {}
            }
        }

        if let PyObjectPayload::Range(rd) = &bbm.receiver.payload {
            let (rs, re, rst) = (rd.start, rd.stop, rd.step);
            match bbm.method_name.as_str() {
                "count" => {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "count() takes exactly one argument",
                        ));
                    }
                    let val = args[0].to_int().unwrap_or(i64::MIN);
                    let found = if rst > 0 {
                        val >= rs && val < re && (val - rs) % rst == 0
                    } else if rst < 0 {
                        val <= rs && val > re && (rs - val) % (-rst) == 0
                    } else {
                        false
                    };
                    return Ok(Some(PyObject::int(if found { 1 } else { 0 })));
                }
                "index" => {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "index() takes exactly one argument",
                        ));
                    }
                    let val = args[0].to_int().unwrap_or(i64::MIN);
                    let in_range = if rst > 0 {
                        val >= rs && val < re && (val - rs) % rst == 0
                    } else if rst < 0 {
                        val <= rs && val > re && (rs - val) % (-rst) == 0
                    } else {
                        false
                    };
                    if in_range {
                        return Ok(Some(PyObject::int((val - rs) / rst)));
                    }
                    return Err(PyException::value_error(format!("{} is not in range", val)));
                }
                _ => {}
            }
        }

        Ok(None)
    }
}
