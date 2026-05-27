use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn sum_items(
        &mut self,
        mut total: PyObjectRef,
        items: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
        if let PyObjectPayload::Int(PyInt::Small(s)) = &total.payload {
            let mut acc: i64 = *s;
            let mut fallback_idx = items.len();
            for (i, item) in items.iter().enumerate() {
                if let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload {
                    acc = acc.wrapping_add(*n);
                } else {
                    total = PyObject::int(acc);
                    total = self.vm_add(&total, item)?;
                    fallback_idx = i + 1;
                    break;
                }
            }
            if fallback_idx < items.len() {
                for item in &items[fallback_idx..] {
                    total = self.vm_add(&total, item)?;
                }
            } else {
                total = PyObject::int(acc);
            }
        } else {
            for item in items {
                total = self.vm_add(&total, item)?;
            }
        }
        Ok(total)
    }
}
