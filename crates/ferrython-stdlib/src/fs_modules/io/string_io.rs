use super::*;

/// StringIO.__init__: installs string buffer methods on self.
/// Called as __init__(self, initial_value="")
pub(super) fn io_string_io_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = optional initial value
    if args.is_empty() {
        return Err(PyException::type_error("StringIO.__init__() requires self"));
    }
    let self_obj = args[0].clone();
    let initial = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        String::new()
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__stringio__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));

        let buf: Rc<PyCell<String>> = Rc::new(PyCell::new(initial));
        let pos: Rc<PyCell<usize>> = Rc::new(PyCell::new(0));

        // write(s) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("StringIO.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() takes 1 argument"));
                }
                let s = a[0].py_to_string();
                let len = s.len();
                let mut bw = b.write();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= bw.len() {
                    bw.push_str(&s);
                } else {
                    let end = cur + len;
                    if end <= bw.len() {
                        bw.replace_range(cur..end, &s);
                    } else {
                        bw.truncate(cur);
                        bw.push_str(&s);
                    }
                }
                *pw = cur + len;
                Ok(PyObject::int(len as i64))
            }),
        );

        // read(size=-1) → str
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("StringIO.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let end = if size < 0 {
                    br.len()
                } else {
                    (cur + size as usize).min(br.len())
                };
                let result = &br[cur..end];
                *pw = end;
                Ok(PyObject::str_val(CompactString::from(result)))
            }),
        );

        // getvalue() → str
        let b = buf.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("StringIO.getvalue", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(b.read().as_str())))
            }),
        );

        // seek(offset, whence=0) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("StringIO.seek", move |a: &[PyObjectRef]| {
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
            PyObject::native_closure("StringIO.tell", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(*p.read() as i64))
            }),
        );

        // truncate(size=None) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("truncate"),
            PyObject::native_closure("StringIO.truncate", move |a: &[PyObjectRef]| {
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

        // readline() → str
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("StringIO.readline", move |_: &[PyObjectRef]| {
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let rest = &br[cur..];
                let end = rest.find('\n').map(|i| cur + i + 1).unwrap_or(br.len());
                *pw = end;
                Ok(PyObject::str_val(CompactString::from(&br[cur..end])))
            }),
        );

        // readlines() → list[str]
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("StringIO.readlines", move |_: &[PyObjectRef]| {
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::list(vec![]));
                }
                let rest = &br[cur..];
                let lines: Vec<PyObjectRef> = rest
                    .split_inclusive('\n')
                    .map(|line| PyObject::str_val(CompactString::from(line)))
                    .collect();
                *pw = br.len();
                Ok(PyObject::list(lines))
            }),
        );

        // close()
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("StringIO.close", move |_| {
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );
        // flush()
        attrs.insert(
            CompactString::from("flush"),
            make_builtin(|_| Ok(PyObject::none())),
        );

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
        attrs.insert(
            CompactString::from("fileno"),
            make_builtin(|_| {
                Err(PyException::runtime_error(
                    "StringIO does not use a file descriptor",
                ))
            }),
        );

        // closed property
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // __enter__ / __exit__ for context manager
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("StringIO.__enter__", move |_: &[PyObjectRef]| {
                Ok(inst_ref.clone())
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );

        // __iter__ — iterates lines
        let rl_buf = buf.clone();
        let rl_pos = pos.clone();
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("StringIO.__iter__", move |_: &[PyObjectRef]| {
                let b = rl_buf.read();
                let p = *rl_pos.read();
                let remaining = if p < b.len() { &b[p..] } else { "" };
                let mut lines: Vec<PyObjectRef> = Vec::new();
                for line in remaining.split('\n') {
                    if !line.is_empty() || lines.is_empty() {
                        lines.push(PyObject::str_val(CompactString::from(format!(
                            "{}\n",
                            line
                        ))));
                    }
                }
                // Fix last line if original didn't end with \n
                if !remaining.ends_with('\n') && !lines.is_empty() {
                    let last_idx = lines.len() - 1;
                    let last = lines[last_idx].py_to_string();
                    lines[last_idx] =
                        PyObject::str_val(CompactString::from(last.trim_end_matches('\n')));
                }
                Ok(PyObject::list(lines))
            }),
        );
    }
    Ok(PyObject::none())
}
