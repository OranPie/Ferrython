//! `io` stdlib module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

mod buffered;
mod bytes_io;
mod string_io;
mod text_io;

use buffered::{io_buffered_reader, io_buffered_writer};
use bytes_io::io_bytes_io_init;
use string_io::io_string_io_init;
use text_io::io_text_io_wrapper_init;

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

/// Helper: call a NativeFunction/NativeClosure directly
fn call_native(func: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Delegate to call_callable which handles native AND Python functions
    ferrython_core::object::call_callable(func, args)
}
