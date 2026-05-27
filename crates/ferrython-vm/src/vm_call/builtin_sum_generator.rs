use ferrython_core::error::PyResult;
use ferrython_core::object::{GeneratorState, PyCell, PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;
use std::rc::Rc;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn sum_generator(
        &mut self,
        mut total: PyObjectRef,
        generator: Rc<PyCell<GeneratorState>>,
    ) -> PyResult<PyObjectRef> {
        if let PyObjectPayload::Int(PyInt::Small(s)) = &total.payload {
            let mut acc: i64 = *s;
            let mut use_native = true;
            loop {
                match self.resume_generator_for_iter(&generator) {
                    Ok(Some(item)) => {
                        if let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload {
                            acc = acc.wrapping_add(*n);
                        } else {
                            total = PyObject::int(acc);
                            total = self.vm_add(&total, &item)?;
                            use_native = false;
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return Err(e),
                }
            }
            if use_native {
                return Ok(PyObject::int(acc));
            }
        }

        loop {
            match self.resume_generator_for_iter(&generator) {
                Ok(Some(item)) => {
                    total = self.vm_add(&total, &item)?;
                }
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }
}
