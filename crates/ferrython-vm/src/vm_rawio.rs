//! RawIOBase helper bridges that need VM method dispatch.

use crate::VirtualMachine;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

impl VirtualMachine {
    /// RawIOBase.read(size=-1): calls self.readinto() to read data.
    pub(crate) fn rawiobase_read(
        &mut self,
        this: &PyObjectRef,
        size: i64,
    ) -> PyResult<PyObjectRef> {
        if size < 0 {
            return self.rawiobase_readall(this);
        }
        let buf = PyObject::wrap(PyObjectPayload::ByteArray(Box::new(vec![
            0u8;
            size as usize
        ])));
        let readinto = self.exec_load_attr_value(this, "readinto")?;
        let n_obj = self.call_object(readinto, vec![buf.clone()])?;
        let n = n_obj.as_int().unwrap_or(0).max(0) as usize;
        if let PyObjectPayload::ByteArray(data) = &buf.payload {
            Ok(PyObject::bytes(data[..n.min(size as usize)].to_vec()))
        } else {
            Ok(PyObject::bytes(vec![]))
        }
    }

    /// RawIOBase.readall(): reads until EOF by calling readinto() in chunks.
    pub(crate) fn rawiobase_readall(&mut self, this: &PyObjectRef) -> PyResult<PyObjectRef> {
        let readinto = self.exec_load_attr_value(this, "readinto")?;
        let mut result = Vec::new();
        loop {
            let buf = PyObject::wrap(PyObjectPayload::ByteArray(Box::new(vec![0u8; 8192])));
            let n_obj = self.call_object(readinto.clone(), vec![buf.clone()])?;
            let n = n_obj.as_int().unwrap_or(0).max(0) as usize;
            if n == 0 {
                break;
            }
            if let PyObjectPayload::ByteArray(data) = &buf.payload {
                result.extend_from_slice(&data[..n.min(data.len())]);
            }
        }
        Ok(PyObject::bytes(result))
    }

    /// Helper to load an attribute via the VM's full resolution.
    fn exec_load_attr_value(&mut self, obj: &PyObjectRef, name: &str) -> PyResult<PyObjectRef> {
        if let Some(val) = obj.get_attr(name) {
            return Ok(val);
        }
        Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'",
            obj.type_name(),
            name
        )))
    }
}
