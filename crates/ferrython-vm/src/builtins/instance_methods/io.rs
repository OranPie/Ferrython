use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

// ── StringIO methods ──

pub(crate) fn call_stringio_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "write" => {
            check_args_min("write", args, 1)?;
            let text = args[0].py_to_string();
            let len = text.len() as i64;
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let mut buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            // Insert/overwrite at position
            if pos >= buf.len() {
                buf.push_str(&text);
            } else {
                let end = (pos + text.len()).min(buf.len());
                buf.replace_range(pos..end, &text);
                if pos + text.len() > end {
                    buf.push_str(&text[end - pos..]);
                }
            }
            let new_pos = pos + text.len();
            attrs.insert(
                CompactString::from("_buffer"),
                PyObject::str_val(CompactString::from(&buf)),
            );
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::int(len))
        }
        "read" => {
            let mut attrs = inst.attrs.write();
            let buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let n = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                args[0].as_int().unwrap_or(-1)
            } else {
                -1
            };
            let result = if n < 0 {
                buf[pos..].to_string()
            } else {
                let end = (pos + n as usize).min(buf.len());
                buf[pos..end].to_string()
            };
            let new_pos = if n < 0 {
                buf.len()
            } else {
                (pos + n as usize).min(buf.len())
            };
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "readline" => {
            let mut attrs = inst.attrs.write();
            let buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let remaining = &buf[pos..];
            let line = if let Some(nl) = remaining.find('\n') {
                &remaining[..=nl]
            } else {
                remaining
            };
            let result = line.to_string();
            attrs.insert(
                CompactString::from("_pos"),
                PyObject::int((pos + result.len()) as i64),
            );
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "readlines" => {
            let mut attrs = inst.attrs.write();
            let buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let remaining = &buf[pos..];
            let lines: Vec<PyObjectRef> = remaining
                .split_inclusive('\n')
                .map(|l| PyObject::str_val(CompactString::from(l)))
                .collect();
            attrs.insert(CompactString::from("_pos"), PyObject::int(buf.len() as i64));
            Ok(PyObject::list(lines))
        }
        "getvalue" => {
            let attrs = inst.attrs.read();
            let buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(&buf)))
        }
        "seek" => {
            check_args_min("seek", args, 1)?;
            let pos = args[0].as_int().unwrap_or(0);
            let whence = if args.len() >= 2 {
                args[1].as_int().unwrap_or(0)
            } else {
                0
            };
            let mut attrs = inst.attrs.write();
            let buf_len = attrs
                .get("_buffer")
                .map(|b| b.py_to_string().len())
                .unwrap_or(0) as i64;
            let new_pos = match whence {
                0 => pos, // SEEK_SET
                1 => {
                    // SEEK_CUR
                    let cur = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0);
                    cur + pos
                }
                2 => buf_len + pos, // SEEK_END
                _ => pos,
            };
            let new_pos = new_pos.max(0);
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos));
            Ok(PyObject::int(new_pos))
        }
        "tell" => {
            let attrs = inst.attrs.read();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0);
            Ok(PyObject::int(pos))
        }
        "truncate" => {
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let size = if !args.is_empty() {
                args[0].as_int().unwrap_or(pos as i64) as usize
            } else {
                pos
            };
            let buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            let truncated: String = buf.chars().take(size).collect();
            attrs.insert(
                CompactString::from("_buffer"),
                PyObject::str_val(CompactString::from(&truncated)),
            );
            Ok(PyObject::int(size as i64))
        }
        "close" => {
            inst.attrs
                .write()
                .insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        "closed" => Ok(inst
            .attrs
            .read()
            .get("_closed")
            .cloned()
            .unwrap_or_else(|| PyObject::bool_val(false))),
        "__enter__" => {
            // Return self — reconstruct from instance data
            let cls = inst.class.clone();
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(new_inst) = &inst_obj.payload {
                *new_inst.attrs.write() = inst.attrs.read().clone();
            }
            Ok(inst_obj)
        }
        "__exit__" => {
            inst.attrs
                .write()
                .insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        "__iter__" => {
            // StringIO is its own iterator — reconstruct self
            let cls = inst.class.clone();
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(new_inst) = &inst_obj.payload {
                *new_inst.attrs.write() = inst.attrs.read().clone();
            }
            Ok(inst_obj)
        }
        "__next__" => {
            // Read next line, raise StopIteration when exhausted
            let mut attrs = inst.attrs.write();
            let buf = attrs
                .get("_buffer")
                .map(|b| b.py_to_string())
                .unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            if pos >= buf.len() {
                return Err(PyException::stop_iteration());
            }
            let remaining = &buf[pos..];
            let line = if let Some(nl) = remaining.find('\n') {
                &remaining[..=nl]
            } else {
                remaining
            };
            let result = line.to_string();
            attrs.insert(
                CompactString::from("_pos"),
                PyObject::int((pos + result.len()) as i64),
            );
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        _ => Err(PyException::attribute_error(format!(
            "'StringIO' object has no attribute '{}'",
            method
        ))),
    }
}

// ── BytesIO methods ──

pub(crate) fn call_bytesio_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "write" => {
            check_args_min("write", args, 1)?;
            let new_bytes = match &args[0].payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
            };
            let len = new_bytes.len() as i64;
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let mut buf = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => (**b).clone(),
                _ => vec![],
            };
            // Extend if needed
            if pos + new_bytes.len() > buf.len() {
                buf.resize(pos + new_bytes.len(), 0);
            }
            buf[pos..pos + new_bytes.len()].copy_from_slice(&new_bytes);
            let new_pos = pos + new_bytes.len();
            attrs.insert(CompactString::from("_buffer"), PyObject::bytes(buf));
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::int(len))
        }
        "read" => {
            let mut attrs = inst.attrs.write();
            let buf = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => (**b).clone(),
                _ => vec![],
            };
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let n = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                args[0].as_int().unwrap_or(-1)
            } else {
                -1
            };
            let result = if n < 0 {
                buf[pos..].to_vec()
            } else {
                let end = (pos + n as usize).min(buf.len());
                buf[pos..end].to_vec()
            };
            let new_pos = if n < 0 {
                buf.len()
            } else {
                (pos + n as usize).min(buf.len())
            };
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::bytes(result))
        }
        "getvalue" => {
            let attrs = inst.attrs.read();
            match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => Ok(PyObject::bytes((**b).clone())),
                _ => Ok(PyObject::bytes(vec![])),
            }
        }
        "seek" => {
            check_args_min("seek", args, 1)?;
            let pos = args[0].as_int().unwrap_or(0);
            let whence = if args.len() >= 2 {
                args[1].as_int().unwrap_or(0)
            } else {
                0
            };
            let mut attrs = inst.attrs.write();
            let buf_len = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => b.len() as i64,
                _ => 0,
            };
            let new_pos = match whence {
                0 => pos,
                1 => attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) + pos,
                2 => buf_len + pos,
                _ => pos,
            };
            let new_pos = new_pos.max(0);
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos));
            Ok(PyObject::int(new_pos))
        }
        "tell" => Ok(PyObject::int(
            inst.attrs
                .read()
                .get("_pos")
                .and_then(|p| p.as_int())
                .unwrap_or(0),
        )),
        "truncate" => {
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let size = if !args.is_empty() {
                args[0].as_int().unwrap_or(pos as i64) as usize
            } else {
                pos
            };
            let buf = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => b[..size.min(b.len())].to_vec(),
                _ => vec![],
            };
            attrs.insert(CompactString::from("_buffer"), PyObject::bytes(buf));
            Ok(PyObject::int(size as i64))
        }
        "close" => {
            inst.attrs
                .write()
                .insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        "__enter__" => {
            let cls = inst.class.clone();
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(new_inst) = &inst_obj.payload {
                *new_inst.attrs.write() = inst.attrs.read().clone();
            }
            Ok(inst_obj)
        }
        "__exit__" => {
            inst.attrs
                .write()
                .insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'BytesIO' object has no attribute '{}'",
            method
        ))),
    }
}
