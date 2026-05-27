use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    InstanceData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use std::rc::Rc;

pub(super) fn compute_hash_digest(algo: &str, data: &[u8]) -> (String, Vec<u8>) {
    use digest::Digest;
    match algo {
        "md5" => {
            let mut h = md5::Md5::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha1" => {
            let mut h = sha1::Sha1::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha224" => {
            let mut h = sha2::Sha224::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha384" => {
            let mut h = sha2::Sha384::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha512" => {
            let mut h = sha2::Sha512::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        _ => {
            // Default to sha256
            let mut h = sha2::Sha256::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
    }
}

pub(crate) fn call_hashlib_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "update" => {
            // Append data to _data buffer, recompute digest lazily
            if args.is_empty() {
                return Err(PyException::type_error("update() takes exactly 1 argument"));
            }
            let new_data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
            };
            let mut w = inst.attrs.write();
            // Append to accumulated data
            let mut accumulated = if let Some(d) = w.get("_data") {
                if let PyObjectPayload::Bytes(b) = &d.payload {
                    (**b).clone()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };
            accumulated.extend_from_slice(&new_data);
            w.insert(
                CompactString::from("_data"),
                PyObject::bytes(accumulated.clone()),
            );
            // Recompute digest
            let algo = if let Some(n) = w.get("name") {
                n.py_to_string()
            } else {
                String::from("sha256")
            };
            let (hex, digest_bytes) = compute_hash_digest(&algo, &accumulated);
            w.insert(
                CompactString::from("_hexdigest"),
                PyObject::str_val(CompactString::from(&hex)),
            );
            w.insert(
                CompactString::from("_digest"),
                PyObject::bytes(digest_bytes),
            );
            Ok(PyObject::none())
        }
        "hexdigest" => {
            let attrs = inst.attrs.read();
            if let Some(hd) = attrs.get("_hexdigest") {
                return Ok(hd.clone());
            }
            Ok(PyObject::str_val(CompactString::from("")))
        }
        "digest" => {
            let attrs = inst.attrs.read();
            if let Some(d) = attrs.get("_digest") {
                return Ok(d.clone());
            }
            Ok(PyObject::bytes(vec![]))
        }
        "copy" => {
            // Return a new hash object with same state
            let attrs = inst.attrs.read();
            let cls = inst.class.clone();
            let class_flags = InstanceData::compute_flags(&cls);
            let new_inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
                Box::new(InstanceData {
                    class: cls,
                    attrs: Rc::new(PyCell::new(attrs.clone())),
                    is_special: true,
                    dict_storage: None,
                    class_flags,
                    finalizer_state: std::cell::Cell::new(0),
                }),
            )));
            Ok(new_inst)
        }
        _ => {
            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                "hash".to_string()
            };
            Err(PyException::attribute_error(format!(
                "'{}' object has no attribute '{}'",
                class_name, method
            )))
        }
    }
}
