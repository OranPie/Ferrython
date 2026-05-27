use super::*;

/// Build a BytesIO instance with methods attached.
/// BytesIO.__init__: installs buffer methods on self.
/// Called as __init__(self, initial_bytes=b"")
pub(super) fn io_bytes_io_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = optional initial bytes
    if args.is_empty() {
        return Err(PyException::type_error("BytesIO.__init__() requires self"));
    }
    let self_obj = args[0].clone();
    let initial = if args.len() > 1 {
        if let PyObjectPayload::Bytes(b) = &args[1].payload {
            (**b).clone()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__bytesio__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));

        let buf: Rc<PyCell<Vec<u8>>> = Rc::new(PyCell::new(initial));
        let pos: Rc<PyCell<usize>> = Rc::new(PyCell::new(0));
        let closed_flag: Rc<PyCell<bool>> = Rc::new(PyCell::new(false));

        // write(b) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("BytesIO.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() takes 1 argument"));
                }
                let data = match &a[0].payload {
                    PyObjectPayload::Bytes(v) => (**v).clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => return Err(PyException::type_error("a bytes-like object is required")),
                };
                let len = data.len();
                let mut bw = b.write();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= bw.len() {
                    bw.extend_from_slice(&data);
                } else {
                    let end = cur + len;
                    if end <= bw.len() {
                        bw[cur..end].copy_from_slice(&data);
                    } else {
                        bw.truncate(cur);
                        bw.extend_from_slice(&data);
                    }
                }
                *pw = cur + len;
                Ok(PyObject::int(len as i64))
            }),
        );

        // read(size=-1) → bytes
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("BytesIO.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::bytes(vec![]));
                }
                let end = if size < 0 {
                    br.len()
                } else {
                    (cur + size as usize).min(br.len())
                };
                let result = br[cur..end].to_vec();
                *pw = end;
                Ok(PyObject::bytes(result))
            }),
        );

        // getvalue() → bytes
        let b = buf.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("BytesIO.getvalue", move |_: &[PyObjectRef]| {
                Ok(PyObject::bytes(b.read().clone()))
            }),
        );

        // seek(offset, whence=0) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BytesIO.seek", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("seek() takes at least 1 argument"));
                }
                let offset = a[0].as_int().unwrap_or(0);
                let whence = if a.len() > 1 {
                    a[1].as_int().unwrap_or(0)
                } else {
                    0
                };
                let br = b.read();
                let mut pw = p.write();
                let new_pos = match whence {
                    0 => offset.max(0) as usize,
                    1 => ((*pw as i64) + offset).max(0) as usize,
                    2 => ((br.len() as i64) + offset).max(0) as usize,
                    _ => return Err(PyException::value_error("invalid whence")),
                };
                *pw = new_pos;
                Ok(PyObject::int(new_pos as i64))
            }),
        );

        // tell() → int
        let p = pos.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BytesIO.tell", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(*p.read() as i64))
            }),
        );

        // truncate(size=None) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("truncate"),
            PyObject::native_closure("BytesIO.truncate", move |a: &[PyObjectRef]| {
                let mut bw = b.write();
                let size = if a.is_empty() || matches!(&a[0].payload, PyObjectPayload::None) {
                    *p.read()
                } else {
                    a[0].as_int().unwrap_or(0) as usize
                };
                bw.truncate(size);
                Ok(PyObject::int(size as i64))
            }),
        );

        // close()
        let cf = closed_flag.clone();
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BytesIO.close", move |_args: &[PyObjectRef]| {
                *cf.write() = true;
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                    d.attrs
                        .write()
                        .insert(CompactString::from("_closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );
        // flush()
        attrs.insert(
            CompactString::from("flush"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // Protocol methods
        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("seekable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("isatty"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );

        // readline()
        let rl_buf = buf.clone();
        let rl_pos = pos.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("BytesIO.readline", move |_: &[PyObjectRef]| {
                let b = rl_buf.read();
                let mut p = rl_pos.write();
                let start = *p;
                if start >= b.len() {
                    return Ok(PyObject::bytes(vec![]));
                }
                let end = b[start..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map(|i| start + i + 1)
                    .unwrap_or(b.len());
                *p = end;
                Ok(PyObject::bytes(b[start..end].to_vec()))
            }),
        );

        // readlines() — read all remaining lines
        let rls_buf = buf.clone();
        let rls_pos = pos.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("BytesIO.readlines", move |_: &[PyObjectRef]| {
                let b = rls_buf.read();
                let mut p = rls_pos.write();
                let mut lines = Vec::new();
                while *p < b.len() {
                    let start = *p;
                    let end = b[start..]
                        .iter()
                        .position(|&c| c == b'\n')
                        .map(|i| start + i + 1)
                        .unwrap_or(b.len());
                    *p = end;
                    lines.push(PyObject::bytes(b[start..end].to_vec()));
                }
                Ok(PyObject::list(lines))
            }),
        );

        // writelines(lines) — write a list of bytes objects
        let wl_buf = buf.clone();
        let wl_pos = pos.clone();
        attrs.insert(
            CompactString::from("writelines"),
            PyObject::native_closure("BytesIO.writelines", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Ok(PyObject::none());
                }
                let items = a[0].to_list()?;
                let mut b = wl_buf.write();
                let mut p = wl_pos.write();
                for item in items {
                    if let PyObjectPayload::Bytes(data) = &item.payload {
                        let d = data;
                        let pos_val = *p;
                        if pos_val == b.len() {
                            b.extend_from_slice(d);
                        } else {
                            let end = (pos_val + d.len()).min(b.len());
                            b.splice(pos_val..end, d.iter().cloned());
                        }
                        *p += d.len();
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__ / __exit__
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BytesIO.__enter__", move |_: &[PyObjectRef]| {
                Ok(inst_ref.clone())
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(PyObject::none())
}
