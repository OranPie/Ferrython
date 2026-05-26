//! `io` stdlib module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

pub fn create_io_module() -> PyObjectRef {
    make_module(
        "io",
        vec![
            ("StringIO", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(io_string_io_init),
                );
                PyObject::class(CompactString::from("StringIO"), vec![], ns)
            }),
            ("BytesIO", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(io_bytes_io_init),
                );
                PyObject::class(CompactString::from("BytesIO"), vec![], ns)
            }),
            ("TextIOWrapper", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(io_text_io_wrapper_init),
                );
                PyObject::class(CompactString::from("TextIOWrapper"), vec![], ns)
            }),
            ("BufferedReader", make_builtin(io_buffered_reader)),
            ("BufferedWriter", make_builtin(io_buffered_writer)),
            (
                "IOBase",
                PyObject::class(CompactString::from("IOBase"), vec![], IndexMap::new()),
            ),
            ("RawIOBase", {
                let mut ns = IndexMap::new();
                // Marker methods — actual logic is handled by VM-level intercept
                ns.insert(
                    CompactString::from("read"),
                    PyObject::native_function("RawIOBase.read", |_| {
                        Err(PyException::runtime_error(
                            "RawIOBase.read requires VM intercept",
                        ))
                    }),
                );
                ns.insert(
                    CompactString::from("readall"),
                    PyObject::native_function("RawIOBase.readall", |_| {
                        Err(PyException::runtime_error(
                            "RawIOBase.readall requires VM intercept",
                        ))
                    }),
                );
                PyObject::class(CompactString::from("RawIOBase"), vec![], ns)
            }),
            (
                "BufferedIOBase",
                PyObject::class(
                    CompactString::from("BufferedIOBase"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            ("BufferedRandom", make_builtin(io_buffered_reader)), // BufferedRandom ≈ BufferedReader for now
            (
                "BufferedRWPair",
                PyObject::class(
                    CompactString::from("BufferedRWPair"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "FileIO",
                PyObject::class(CompactString::from("FileIO"), vec![], IndexMap::new()),
            ),
            (
                "TextIOBase",
                PyObject::class(CompactString::from("TextIOBase"), vec![], IndexMap::new()),
            ),
            (
                "UnsupportedOperation",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            ("SEEK_SET", PyObject::int(0)),
            ("SEEK_CUR", PyObject::int(1)),
            ("SEEK_END", PyObject::int(2)),
            ("DEFAULT_BUFFER_SIZE", PyObject::int(8192)),
            // io.text_encoding(encoding, stacklevel=2) — Python 3.11+
            (
                "text_encoding",
                make_builtin(|args: &[PyObjectRef]| {
                    // If encoding is None or not provided, return "locale" (CPython default)
                    if args.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("locale")));
                    }
                    if matches!(&args[0].payload, PyObjectPayload::None) {
                        return Ok(PyObject::str_val(CompactString::from("locale")));
                    }
                    Ok(args[0].clone())
                }),
            ),
            (
                "open",
                make_builtin(|args| {
                    // io.open — replicates builtins.open() behavior
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "open() requires at least 1 argument",
                        ));
                    }
                    let path = args[0].py_to_string();
                    let mode = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "r".to_string()
                    };
                    let is_binary = mode.contains('b');
                    let is_write = mode.contains('w') || mode.contains('a') || mode.contains('x');

                    let content = if is_write {
                        if mode.contains('a') {
                            std::fs::read_to_string(&path).unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        std::fs::read_to_string(&path)
                            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?
                    };

                    let data: Rc<PyCell<(String, usize, bool)>> =
                        Rc::new(PyCell::new((content, 0, false)));
                    let cls =
                        PyObject::class(CompactString::from("_io_file"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut a = d.attrs.write();
                        a.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from(path.as_str())),
                        );
                        a.insert(
                            CompactString::from("mode"),
                            PyObject::str_val(CompactString::from(mode.as_str())),
                        );
                        a.insert(CompactString::from("closed"), PyObject::bool_val(false));
                        let d1 = data.clone();
                        a.insert(
                            CompactString::from("read"),
                            PyObject::native_closure("read", move |rargs| {
                                let g = d1.read();
                                let remaining = &g.0[g.1..];
                                let n = rargs.first().and_then(|a| a.as_int());
                                let text = match n {
                                    Some(n) if n >= 0 => {
                                        let end = (g.1 + n as usize).min(g.0.len());
                                        g.0[g.1..end].to_string()
                                    }
                                    _ => remaining.to_string(),
                                };
                                drop(g);
                                let len = text.len();
                                d1.write().1 += len;
                                if is_binary {
                                    Ok(PyObject::bytes(text.into_bytes()))
                                } else {
                                    Ok(PyObject::str_val(CompactString::from(text)))
                                }
                            }),
                        );
                        let d2 = data.clone();
                        a.insert(
                            CompactString::from("readline"),
                            PyObject::native_closure("readline", move |_| {
                                let g = d2.read();
                                let remaining = &g.0[g.1..];
                                if remaining.is_empty() {
                                    return Ok(PyObject::str_val(CompactString::from("")));
                                }
                                let line = if let Some(idx) = remaining.find('\n') {
                                    &remaining[..=idx]
                                } else {
                                    remaining
                                };
                                let r = line.to_string();
                                drop(g);
                                d2.write().1 += r.len();
                                Ok(PyObject::str_val(CompactString::from(r)))
                            }),
                        );
                        let d3 = data.clone();
                        let p2 = path.clone();
                        let m2 = mode.clone();
                        a.insert(
                            CompactString::from("write"),
                            PyObject::native_closure("write", move |wargs| {
                                if wargs.is_empty() {
                                    return Err(PyException::type_error("write requires data"));
                                }
                                let text = wargs[wargs.len() - 1].py_to_string();
                                let len = text.len();
                                d3.write().0.push_str(&text);
                                // Write to disk
                                let g = d3.read();
                                if m2.contains('a') {
                                    use std::io::Write;
                                    let mut f = std::fs::OpenOptions::new()
                                        .append(true)
                                        .create(true)
                                        .open(&p2)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                    f.write_all(text.as_bytes())
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                } else {
                                    std::fs::write(&p2, &g.0)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                }
                                drop(g);
                                Ok(PyObject::int(len as i64))
                            }),
                        );
                        let d4 = data.clone();
                        let inst_for_close = inst.clone();
                        a.insert(
                            CompactString::from("close"),
                            PyObject::native_closure("close", move |_| {
                                d4.write().2 = true;
                                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                                    d.attrs.write().insert(
                                        CompactString::from("closed"),
                                        PyObject::bool_val(true),
                                    );
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        a.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("__enter__", {
                                let inst2 = inst.clone();
                                move |_| Ok(inst2.clone())
                            }),
                        );
                        let inst_for_exit = inst.clone();
                        let d5 = data.clone();
                        a.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("__exit__", move |_| {
                                d5.write().2 = true;
                                if let PyObjectPayload::Instance(ref d) = inst_for_exit.payload {
                                    d.attrs.write().insert(
                                        CompactString::from("closed"),
                                        PyObject::bool_val(true),
                                    );
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        let d6 = data.clone();
                        a.insert(
                            CompactString::from("seek"),
                            PyObject::native_closure("seek", move |sargs| {
                                let pos =
                                    sargs.first().and_then(|a| a.as_int()).unwrap_or(0) as usize;
                                d6.write().1 = pos;
                                Ok(PyObject::int(pos as i64))
                            }),
                        );
                        let d7 = data.clone();
                        a.insert(
                            CompactString::from("tell"),
                            PyObject::native_closure("tell", move |_| {
                                Ok(PyObject::int(d7.read().1 as i64))
                            }),
                        );
                        a.insert(
                            CompactString::from("flush"),
                            make_builtin(|_| Ok(PyObject::none())),
                        );
                        a.insert(
                            CompactString::from("readable"),
                            PyObject::native_closure("readable", {
                                let m = mode.clone();
                                move |_| Ok(PyObject::bool_val(m.contains('r')))
                            }),
                        );
                        a.insert(
                            CompactString::from("writable"),
                            PyObject::native_closure("writable", {
                                let m = mode.clone();
                                move |_| Ok(PyObject::bool_val(is_write || m.contains('+')))
                            }),
                        );
                        a.insert(
                            CompactString::from("seekable"),
                            make_builtin(|_| Ok(PyObject::bool_val(true))),
                        );
                        a.insert(
                            CompactString::from("isatty"),
                            make_builtin(|_| Ok(PyObject::bool_val(false))),
                        );
                        // fileno() — open a real OS fd for the path so mmap etc. can work
                        let fpath = path.clone();
                        let fmode = mode.clone();
                        a.insert(
                            CompactString::from("fileno"),
                            PyObject::native_closure("fileno", move |_: &[PyObjectRef]| {
                                use std::os::unix::io::IntoRawFd;
                                let f = if fmode.contains('w') || fmode.contains('a') {
                                    std::fs::OpenOptions::new()
                                        .read(true)
                                        .write(true)
                                        .open(&fpath)
                                } else {
                                    std::fs::File::open(&fpath)
                                };
                                match f {
                                    Ok(file) => Ok(PyObject::int(file.into_raw_fd() as i64)),
                                    Err(e) => {
                                        Err(PyException::os_error(format!("{}: '{}'", e, fpath)))
                                    }
                                }
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "FileIO",
                make_builtin(|args| {
                    // FileIO(name, mode='r') -- thin wrapper around OS file descriptor
                    if args.is_empty() {
                        return Err(PyException::type_error("FileIO requires a file path or fd"));
                    }
                    let name = args[0].py_to_string();
                    let mode = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "r".to_string()
                    };
                    let file = if mode.contains('w') {
                        std::fs::File::create(&name)
                            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, name)))?
                    } else {
                        std::fs::File::open(&name)
                            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, name)))?
                    };
                    let buf: Rc<PyCell<Option<std::fs::File>>> = Rc::new(PyCell::new(Some(file)));
                    let cls =
                        PyObject::class(CompactString::from("FileIO"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(d) = &inst.payload {
                        let mut a = d.attrs.write();
                        a.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from(name)),
                        );
                        a.insert(
                            CompactString::from("mode"),
                            PyObject::str_val(CompactString::from(mode.as_str())),
                        );
                        a.insert(CompactString::from("closed"), PyObject::bool_val(false));
                        let buf2 = buf.clone();
                        a.insert(
                            CompactString::from("read"),
                            PyObject::native_closure("FileIO.read", move |_| {
                                use std::io::Read;
                                let mut guard = buf2.write();
                                if let Some(ref mut f) = *guard {
                                    let mut data = Vec::new();
                                    f.read_to_end(&mut data)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                    Ok(PyObject::bytes(data))
                                } else {
                                    Err(PyException::value_error("I/O operation on closed file"))
                                }
                            }),
                        );
                        let buf3 = buf.clone();
                        a.insert(
                            CompactString::from("write"),
                            PyObject::native_closure("FileIO.write", move |wargs| {
                                use std::io::Write;
                                if wargs.is_empty() {
                                    return Err(PyException::type_error("write requires data"));
                                }
                                let mut guard = buf3.write();
                                if let Some(ref mut f) = *guard {
                                    let data = match &wargs[0].payload {
                                        PyObjectPayload::Bytes(b) => (**b).clone(),
                                        _ => wargs[0].py_to_string().into_bytes(),
                                    };
                                    let n = f
                                        .write(&data)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                    Ok(PyObject::int(n as i64))
                                } else {
                                    Err(PyException::value_error("I/O operation on closed file"))
                                }
                            }),
                        );
                        let buf4 = buf.clone();
                        a.insert(
                            CompactString::from("close"),
                            PyObject::native_closure("FileIO.close", move |_| {
                                *buf4.write() = None;
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
        ],
    )
}

/// StringIO.__init__: installs string buffer methods on self.
/// Called as __init__(self, initial_value="")
fn io_string_io_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

/// Build a BytesIO instance with methods attached.
/// BytesIO.__init__: installs buffer methods on self.
/// Called as __init__(self, initial_bytes=b"")
fn io_bytes_io_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

/// TextIOWrapper.__init__: installs buffer-delegating methods on self.
/// Called as __init__(self, buffer, encoding='utf-8', errors='strict', ...)
fn io_text_io_wrapper_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = buffer, optional encoding/kwargs
    if args.len() < 2 {
        return Err(PyException::type_error(
            "TextIOWrapper.__init__() requires a buffer argument",
        ));
    }
    let self_obj = args[0].clone();
    let buffer = args[1].clone();
    let encoding = if args.len() > 2 {
        args[2].py_to_string()
    } else {
        "utf-8".to_string()
    };
    // Extract kwargs if trailing dict
    let (enc, _errors) = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            let e = r
                .get(&HashableKey::str_key(CompactString::from("encoding")))
                .map(|v| v.py_to_string())
                .unwrap_or(encoding);
            let er = r
                .get(&HashableKey::str_key(CompactString::from("errors")))
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| "strict".to_string());
            (e, er)
        } else {
            (encoding, "strict".to_string())
        }
    } else {
        (encoding, "strict".to_string())
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("buffer"), buffer.clone());
        attrs.insert(
            CompactString::from("encoding"),
            PyObject::str_val(CompactString::from(&enc)),
        );
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from("r")),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from("<TextIOWrapper>")),
        );

        // read(size=-1) — decode bytes from buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("TextIOWrapper.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                if let Some(read_fn) = buf.get_attr("read") {
                    let bytes_result = if size < 0 {
                        call_native(&read_fn, &[])?
                    } else {
                        call_native(&read_fn, &[PyObject::int(size)])?
                    };
                    if let PyObjectPayload::Bytes(b) = &bytes_result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(bytes_result)
                    }
                } else {
                    Err(PyException::type_error("buffer has no read method"))
                }
            }),
        );

        // write(s) — encode str to bytes and write to buffer (rejects bytes like CPython)
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("TextIOWrapper.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() requires 1 argument"));
                }
                // TextIOWrapper only accepts str, not bytes
                if matches!(&a[0].payload, PyObjectPayload::Bytes(_)) {
                    return Err(PyException::type_error(
                        "write() argument must be str, not bytes",
                    ));
                }
                let text = a[0].py_to_string();
                let bytes_obj = PyObject::bytes(text.as_bytes().to_vec());
                if let Some(write_fn) = buf.get_attr("write") {
                    call_native(&write_fn, &[bytes_obj])
                } else {
                    Err(PyException::type_error("buffer has no write method"))
                }
            }),
        );

        // readline() — read line from buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("TextIOWrapper.readline", move |_: &[PyObjectRef]| {
                if let Some(readline_fn) = buf.get_attr("readline") {
                    let result = call_native(&readline_fn, &[])?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(PyException::type_error("buffer has no readline method"))
                }
            }),
        );

        // readlines(hint=-1) — read all lines
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("TextIOWrapper.readlines", move |a: &[PyObjectRef]| {
                let hint = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let mut lines = Vec::new();
                let mut total_bytes = 0i64;
                loop {
                    if let Some(readline_fn) = buf.get_attr("readline") {
                        let result = call_native(&readline_fn, &[])?;
                        let line_str = if let PyObjectPayload::Bytes(b) = &result.payload {
                            String::from_utf8_lossy(b).to_string()
                        } else {
                            result.py_to_string()
                        };
                        if line_str.is_empty() {
                            break;
                        }
                        total_bytes += line_str.len() as i64;
                        lines.push(PyObject::str_val(CompactString::from(line_str)));
                        if hint > 0 && total_bytes >= hint {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                Ok(PyObject::list(lines))
            }),
        );

        // writelines(lines) — write an iterable of strings
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("writelines"),
            PyObject::native_closure("TextIOWrapper.writelines", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writelines() requires 1 argument"));
                }
                if let Some(write_fn) = buf.get_attr("write") {
                    if let PyObjectPayload::List(items) = &a[0].payload {
                        for item in items.read().iter() {
                            let text = item.py_to_string();
                            let bytes_obj = PyObject::bytes(text.as_bytes().to_vec());
                            call_native(&write_fn, &[bytes_obj])?;
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // seek/tell — delegate to buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("TextIOWrapper.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = buf.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("TextIOWrapper.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = buf.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        // flush — delegate to buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("TextIOWrapper.flush", move |_: &[PyObjectRef]| {
                if let Some(flush_fn) = buf.get_attr("flush") {
                    call_native(&flush_fn, &[])
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        // readable/writable/seekable
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

        // close — delegate to buffer and mark closed
        let buf = buffer.clone();
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("TextIOWrapper.close", move |_| {
                if let Some(close_fn) = buf.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__ / __exit__
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("TextIOWrapper.__enter__", move |_| Ok(inst_ref.clone())),
        );
        let inst_for_exit = self_obj.clone();
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("TextIOWrapper.__exit__", move |_| {
                if let Some(close_fn) = buf.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_exit.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::bool_val(false))
            }),
        );

        // getvalue() — delegate to buffer (common for StringIO/BytesIO wrappers)
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("TextIOWrapper.getvalue", move |_: &[PyObjectRef]| {
                if let Some(gv) = buf.get_attr("getvalue") {
                    let result = call_native(&gv, &[])?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(PyException::attribute_error(
                        "underlying buffer has no getvalue",
                    ))
                }
            }),
        );
    }
    Ok(PyObject::none())
}

/// BufferedReader: wraps a raw binary stream with buffering
fn io_buffered_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "BufferedReader() requires a raw stream",
        ));
    }
    let raw = args[0].clone();
    let cls = PyObject::class(
        CompactString::from("BufferedReader"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("raw"), raw.clone());

        let r = raw.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("BufferedReader.read", move |a: &[PyObjectRef]| {
                if let Some(read_fn) = r.get_attr("read") {
                    call_native(&read_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no read method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("BufferedReader.readline", move |a: &[PyObjectRef]| {
                if let Some(readline_fn) = r.get_attr("readline") {
                    call_native(&readline_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no readline method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("BufferedReader.readlines", move |_: &[PyObjectRef]| {
                let mut lines = Vec::new();
                loop {
                    if let Some(readline_fn) = r.get_attr("readline") {
                        let result = call_native(&readline_fn, &[])?;
                        let is_empty = match &result.payload {
                            PyObjectPayload::Bytes(b) => b.is_empty(),
                            _ => result.py_to_string().is_empty(),
                        };
                        if is_empty {
                            break;
                        }
                        lines.push(result);
                    } else {
                        break;
                    }
                }
                Ok(PyObject::list(lines))
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BufferedReader.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = r.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BufferedReader.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = r.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        let inst_for_close = inst.clone();
        let r = raw.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BufferedReader.close", move |_| {
                if let Some(close_fn) = r.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        let inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BufferedReader.__enter__", move |_| Ok(inst_ref.clone())),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(inst)
}

/// BufferedWriter: wraps a raw binary stream with write buffering
fn io_buffered_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "BufferedWriter() requires a raw stream",
        ));
    }
    let raw = args[0].clone();
    let cls = PyObject::class(
        CompactString::from("BufferedWriter"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("raw"), raw.clone());

        let r = raw.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("BufferedWriter.write", move |a: &[PyObjectRef]| {
                if let Some(write_fn) = r.get_attr("write") {
                    call_native(&write_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no write method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("BufferedWriter.flush", move |_: &[PyObjectRef]| {
                if let Some(flush_fn) = r.get_attr("flush") {
                    call_native(&flush_fn, &[])
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BufferedWriter.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = r.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BufferedWriter.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = r.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        let inst_for_close = inst.clone();
        let r = raw;
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BufferedWriter.close", move |_| {
                if let Some(flush_fn) = r.get_attr("flush") {
                    let _ = call_native(&flush_fn, &[]);
                }
                if let Some(close_fn) = r.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        let inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BufferedWriter.__enter__", move |_| Ok(inst_ref.clone())),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(inst)
}

/// Helper: call a NativeFunction/NativeClosure directly
fn call_native(func: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Delegate to call_callable which handles native AND Python functions
    ferrython_core::object::call_callable(func, args)
}
