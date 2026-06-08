use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

/// `marshal` — internal Python object serialization
pub fn create_marshal_module() -> PyObjectRef {
    let dumps_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("marshal.dumps", args, 1)?;
        fn marshal_encode(obj: &PyObjectRef) -> Vec<u8> {
            match &obj.payload {
                PyObjectPayload::None => vec![b'N'],
                PyObjectPayload::Bool(b) => {
                    if *b {
                        vec![b'T']
                    } else {
                        vec![b'F']
                    }
                }
                PyObjectPayload::Int(n) => {
                    let mut buf = vec![b'i'];
                    buf.extend_from_slice(&n.to_i64().unwrap_or(0).to_le_bytes());
                    buf
                }
                PyObjectPayload::Float(f) => {
                    let mut buf = vec![b'g'];
                    buf.extend_from_slice(&f.to_le_bytes());
                    buf
                }
                PyObjectPayload::Str(s) => {
                    let bytes = s.as_bytes();
                    let mut buf = vec![b's'];
                    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(bytes);
                    buf
                }
                PyObjectPayload::Bytes(b) => {
                    let mut buf = vec![b'z'];
                    buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
                    buf.extend_from_slice(b);
                    buf
                }
                PyObjectPayload::List(items) => {
                    let items = items.read();
                    let mut buf = vec![b'['];
                    buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
                    for item in items.iter() {
                        buf.extend(marshal_encode(item));
                    }
                    buf
                }
                PyObjectPayload::Tuple(items) => {
                    let mut buf = vec![b'('];
                    buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
                    for item in items.iter() {
                        buf.extend(marshal_encode(item));
                    }
                    buf
                }
                PyObjectPayload::Dict(map) => {
                    let map = map.read();
                    let mut buf = vec![b'{'];
                    buf.extend_from_slice(&(map.len() as u32).to_le_bytes());
                    for (k, v) in map.iter() {
                        let key_obj = match k {
                            HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
                            HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
                            HashableKey::Bool(b) => PyObject::bool_val(*b),
                            _ => PyObject::none(),
                        };
                        buf.extend(marshal_encode(&key_obj));
                        buf.extend(marshal_encode(v));
                    }
                    buf
                }
                _ => vec![b'N'],
            }
        }
        Ok(PyObject::bytes(marshal_encode(&args[0])))
    });
    let loads_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("marshal.loads", args, 1)?;
        let data = match &args[0].payload {
            PyObjectPayload::Bytes(b) => (**b).clone(),
            _ => return Err(PyException::type_error("marshal.loads requires bytes")),
        };
        fn marshal_decode(data: &[u8], pos: &mut usize) -> PyResult<PyObjectRef> {
            if *pos >= data.len() {
                return Err(PyException::eof_error("EOF read where object expected"));
            }
            let tag = data[*pos];
            *pos += 1;
            match tag {
                b'N' => Ok(PyObject::none()),
                b'T' => Ok(PyObject::bool_val(true)),
                b'F' => Ok(PyObject::bool_val(false)),
                b'i' => {
                    if *pos + 8 > data.len() {
                        return Err(PyException::value_error("marshal: truncated"));
                    }
                    let n = i64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
                    *pos += 8;
                    Ok(PyObject::int(n))
                }
                b'g' => {
                    if *pos + 8 > data.len() {
                        return Err(PyException::value_error("marshal: truncated"));
                    }
                    let f = f64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
                    *pos += 8;
                    Ok(PyObject::float(f))
                }
                b's' | b'z' => {
                    if *pos + 4 > data.len() {
                        return Err(PyException::value_error("marshal: truncated"));
                    }
                    let len = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
                    *pos += 4;
                    if *pos + len > data.len() {
                        return Err(PyException::value_error("marshal: truncated"));
                    }
                    let slice = data[*pos..*pos + len].to_vec();
                    *pos += len;
                    if tag == b's' {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(&slice).as_ref(),
                        )))
                    } else {
                        Ok(PyObject::bytes(slice))
                    }
                }
                b'[' | b'(' => {
                    if *pos + 4 > data.len() {
                        return Err(PyException::value_error("marshal: truncated"));
                    }
                    let len = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
                    *pos += 4;
                    let mut items = Vec::with_capacity(len);
                    for _ in 0..len {
                        items.push(marshal_decode(data, pos)?);
                    }
                    if tag == b'[' {
                        Ok(PyObject::list(items))
                    } else {
                        Ok(PyObject::tuple(items))
                    }
                }
                b'{' => {
                    if *pos + 4 > data.len() {
                        return Err(PyException::value_error("marshal: truncated"));
                    }
                    let len = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
                    *pos += 4;
                    let mut map = IndexMap::new();
                    for _ in 0..len {
                        let k = marshal_decode(data, pos)?;
                        let v = marshal_decode(data, pos)?;
                        let key = match &k.payload {
                            PyObjectPayload::Str(s) => HashableKey::str_key(s.to_compact_string()),
                            PyObjectPayload::Int(n) => HashableKey::Int(n.clone()),
                            PyObjectPayload::Bool(b) => HashableKey::Bool(*b),
                            _ => HashableKey::str_key(CompactString::from(k.py_to_string())),
                        };
                        map.insert(key, v);
                    }
                    Ok(PyObject::dict(map))
                }
                _ => Err(PyException::value_error(format!(
                    "marshal: unknown tag {}",
                    tag
                ))),
            }
        }
        let mut pos = 0;
        marshal_decode(&data, &mut pos)
    });
    make_module(
        "marshal",
        vec![
            ("dumps", dumps_fn),
            ("loads", loads_fn),
            (
                "dump",
                make_builtin(|_| {
                    Err(PyException::type_error(
                        "marshal.dump() requires a file object",
                    ))
                }),
            ),
            (
                "load",
                make_builtin(|_| {
                    Err(PyException::type_error(
                        "marshal.load() requires a file object",
                    ))
                }),
            ),
            ("version", PyObject::int(4)),
        ],
    )
}
