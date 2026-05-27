use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    guard_eager_allocation, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn bytes_like_data(obj: &PyObjectRef) -> Option<Vec<u8>> {
        match &obj.payload {
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some((**b).clone()),
            PyObjectPayload::Instance(inst) => {
                if inst.attrs.read().contains_key("__memoryview__") {
                    if let Some(base) = inst.attrs.read().get("obj").cloned() {
                        return Self::bytes_like_data(&base);
                    }
                }
                if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                    return Self::bytes_like_data(&value);
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn bytes_size_from_index(obj: &PyObjectRef) -> PyResult<usize> {
        let index = obj.to_index()?;
        let n = index.to_i64().ok_or_else(|| {
            PyException::overflow_error("cannot fit 'int' into an index-sized integer")
        })?;
        if isize::try_from(n).is_err() {
            return Err(PyException::overflow_error(
                "cannot fit 'int' into an index-sized integer",
            ));
        }
        if n < 0 {
            return Err(PyException::value_error("negative count"));
        }
        Ok(n as usize)
    }

    pub(super) fn byte_from_index(obj: &PyObjectRef) -> PyResult<u8> {
        let index = obj.to_index().map_err(|e| {
            if e.kind == ExceptionKind::TypeError {
                PyException::type_error("an integer is required")
            } else {
                e
            }
        })?;
        let Some(n) = index.to_i64() else {
            return Err(PyException::value_error("bytes must be in range(0, 256)"));
        };
        if !(0..=255).contains(&n) {
            return Err(PyException::value_error("bytes must be in range(0, 256)"));
        }
        Ok(n as u8)
    }

    pub(super) fn bytes_from_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<u8>> {
        if let PyObjectPayload::List(items) = &obj.payload {
            let mut result = Vec::new();
            let mut index = 0usize;
            loop {
                let item = {
                    let read = items.read();
                    if index >= read.len() {
                        break;
                    }
                    read[index].clone()
                };
                guard_eager_allocation(result.len().saturating_add(1), "bytes iterable")?;
                result.push(Self::byte_from_index(&item)?);
                index += 1;
            }
            return Ok(result);
        }

        let items = self.collect_iterable(obj)?;
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            result.push(Self::byte_from_index(&item)?);
        }
        Ok(result)
    }

    pub(super) fn vm_bytes_constructor(
        &mut self,
        args: &[PyObjectRef],
        mutable: bool,
    ) -> PyResult<PyObjectRef> {
        let make = |data: Vec<u8>| {
            if mutable {
                PyObject::bytearray(data)
            } else {
                PyObject::bytes(data)
            }
        };
        if args.is_empty() {
            return Ok(make(Vec::new()));
        }
        if args.len() > 3 {
            return Err(PyException::type_error(format!(
                "{}() takes at most 3 arguments ({} given)",
                if mutable { "bytearray" } else { "bytes" },
                args.len()
            )));
        }

        let source = &args[0];
        if args.len() > 1 {
            if let PyObjectPayload::Str(s) = &source.payload {
                let encoded = builtins::call_str_method(s.as_str(), "encode", &args[1..])?;
                if let PyObjectPayload::Bytes(b) = &encoded.payload {
                    return Ok(make((**b).to_vec()));
                }
                return Err(PyException::type_error(
                    "string encode did not return bytes",
                ));
            }
            return Err(PyException::type_error(
                "encoding without a string argument",
            ));
        }

        if let Some(data) = Self::bytes_like_data(source) {
            return Ok(make(data));
        }

        if !mutable {
            let source = if let PyObjectPayload::Instance(inst) = &source.payload {
                if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                    if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                        (nc.func)(&[])?
                    } else {
                        source.clone()
                    }
                } else {
                    source.clone()
                }
            } else {
                source.clone()
            };
            if let PyObjectPayload::Instance(_) = &source.payload {
                if let Some(method) = Self::resolve_instance_dunder(&source, "__bytes__") {
                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![source]
                    };
                    let result = self.call_object(method, ca)?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        return Ok(PyObject::bytes((**b).to_vec()));
                    }
                    return Err(PyException::type_error("__bytes__ returned non-bytes"));
                }
            }
        }

        let has_index = matches!(
            source.payload,
            PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
        ) || source.get_attr("__index__").is_some();
        if has_index {
            let size = Self::bytes_size_from_index(source)?;
            guard_eager_allocation(size, if mutable { "bytearray()" } else { "bytes()" })?;
            return Ok(make(vec![0u8; size]));
        }

        Ok(make(self.bytes_from_iterable(source)?))
    }
}
