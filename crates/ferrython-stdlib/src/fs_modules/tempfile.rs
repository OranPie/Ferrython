use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Mutex;

// ── tempfile module (basic) ──

use std::sync::atomic::{AtomicU64, Ordering};

static TMPFILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Shared write buffers for NamedTemporaryFile instances, keyed by path.
#[allow(dead_code)]
static TMPFILE_BUFFERS: std::sync::LazyLock<Mutex<IndexMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

#[cfg(unix)]
fn read_from_fd(fd: i32, size: isize, is_binary: bool) -> PyResult<PyObjectRef> {
    let buf = if size < 0 {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        loop {
            let n = unsafe { libc::read(fd, tmp.as_mut_ptr() as *mut libc::c_void, tmp.len()) };
            if n <= 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n as usize]);
        }
        buf
    } else {
        let mut buf = vec![0u8; size as usize];
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            return Err(PyException::os_error("read failed".to_string()));
        }
        buf.truncate(n as usize);
        buf
    };
    if is_binary {
        Ok(PyObject::bytes(buf))
    } else {
        Ok(PyObject::str_val(CompactString::from(
            String::from_utf8_lossy(&buf).as_ref(),
        )))
    }
}

#[cfg(unix)]
fn readline_from_fd(fd: i32, is_binary: bool) -> PyResult<PyObjectRef> {
    let mut buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        let n = unsafe { libc::read(fd, byte.as_mut_ptr() as *mut libc::c_void, 1) };
        if n < 0 {
            return Err(PyException::os_error("read failed".to_string()));
        }
        if n == 0 {
            break;
        }
        buf.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    if is_binary {
        Ok(PyObject::bytes(buf))
    } else {
        Ok(PyObject::str_val(CompactString::from(
            String::from_utf8_lossy(&buf).as_ref(),
        )))
    }
}

fn named_temporary_file(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract keyword args (mode, suffix, prefix, delete)
    let mut mode = String::from("w+b");
    let mut suffix = String::from("");
    let mut delete = true;
    if let Some(first) = args.first() {
        if !matches!(&first.payload, PyObjectPayload::Dict(_)) {
            mode = first.py_to_string();
        }
    }
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(d) = &last.payload {
            let d = d.read();
            for (k, v) in d.iter() {
                let key_s = match k {
                    HashableKey::Str(s) => s.as_str().to_string(),
                    _ => continue,
                };
                match key_s.as_str() {
                    "mode" => mode = v.py_to_string(),
                    "suffix" => suffix = v.py_to_string(),
                    "prefix" => { /* ignored for now */ }
                    "delete" => delete = v.is_truthy(),
                    _ => {}
                }
            }
        }
    }

    let n = TMPFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ferrython_ntf_{}{}{}",
        std::process::id(),
        n,
        suffix
    ));
    let path_str = path.to_string_lossy().to_string();
    let is_binary = mode.contains('b');

    // Open with read+write so both directions work
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| PyException::runtime_error(format!("tempfile: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::io::IntoRawFd;

        let fd = file.into_raw_fd();
        let state = Rc::new(PyCell::new((fd, false))); // (fd, closed)
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(&path_str)),
        );
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from(&mode)),
        );
        attrs.insert(CompactString::from("_delete"), PyObject::bool_val(delete));
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // write(data)
        let s1 = state.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("write", move |a| {
                let g = s1.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let data_arg = if a.len() > 1 {
                    &a[1]
                } else if !a.is_empty() {
                    &a[0]
                } else {
                    return Err(PyException::type_error("write requires data"));
                };
                let data_bytes = match &data_arg.payload {
                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => data_arg.py_to_string().into_bytes(),
                };
                let n = unsafe {
                    libc::write(
                        fd,
                        data_bytes.as_ptr() as *const libc::c_void,
                        data_bytes.len(),
                    )
                };
                if n < 0 {
                    return Err(PyException::os_error("write failed".to_string()));
                }
                Ok(PyObject::int(n as i64))
            }),
        );

        // read([size])
        let s2 = state.clone();
        let is_bin_r = is_binary;
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("read", move |a| {
                let g = s2.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let size: isize = if a.len() > 1 {
                    a[1].as_int().unwrap_or(-1) as isize
                } else if !a.is_empty() {
                    a[0].as_int().unwrap_or(-1) as isize
                } else {
                    -1
                };
                read_from_fd(fd, size, is_bin_r)
            }),
        );

        // readline()
        let s2_line = state.clone();
        let is_bin_line = is_binary;
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("readline", move |_a| {
                let g = s2_line.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                readline_from_fd(g.0, is_bin_line)
            }),
        );

        // seek(offset, whence=0)
        let s3 = state.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("seek", move |a| {
                let g = s3.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let offset = if a.len() > 1 {
                    a[1].as_int().unwrap_or(0) as i64
                } else if !a.is_empty() {
                    a[0].as_int().unwrap_or(0) as i64
                } else {
                    0i64
                };
                let whence = if a.len() > 2 {
                    a[2].as_int().unwrap_or(0) as i32
                } else {
                    0i32
                };
                let pos = unsafe { libc::lseek(fd, offset as libc::off_t, whence) };
                if pos < 0 {
                    return Err(PyException::os_error("seek failed".to_string()));
                }
                Ok(PyObject::int(pos as i64))
            }),
        );

        // tell()
        let s4 = state.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("tell", move |_a| {
                let g = s4.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let pos = unsafe { libc::lseek(g.0, 0, libc::SEEK_CUR) };
                Ok(PyObject::int(pos as i64))
            }),
        );

        // flush()
        let s5 = state.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("flush", move |_a| {
                let g = s5.read();
                if !g.1 {
                    unsafe {
                        libc::fsync(g.0);
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // close()
        let s6 = state.clone();
        let ps_c = path_str.clone();
        let del_c = delete;
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("close", move |_| {
                let mut g = s6.write();
                if !g.1 {
                    g.1 = true;
                    unsafe {
                        libc::close(g.0);
                    }
                    if del_c {
                        std::fs::remove_file(&ps_c).ok();
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__(self)
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_function("__enter__", |args| {
                if !args.is_empty() {
                    Ok(args[0].clone())
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        // __exit__ — close + optionally delete
        let s7 = state.clone();
        let ps_e = path_str.clone();
        let del_e = delete;
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_| {
                let mut g = s7.write();
                if !g.1 {
                    g.1 = true;
                    unsafe {
                        libc::close(g.0);
                    }
                    if del_e {
                        std::fs::remove_file(&ps_e).ok();
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );

        attrs.insert(
            CompactString::from("_bind_methods"),
            PyObject::bool_val(true),
        );

        let s_iter = state.clone();
        let is_bin_iter = is_binary;
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("__iter__", move |_| {
                let g = s_iter.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let mut items = Vec::new();
                loop {
                    let line = readline_from_fd(fd, is_bin_iter)?;
                    let is_empty = match &line.payload {
                        PyObjectPayload::Str(s) => s.is_empty(),
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.is_empty(),
                        _ => false,
                    };
                    if is_empty {
                        break;
                    }
                    items.push(line);
                }
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::List { items, index: 0 }),
                ))))
            }),
        );

        let class = PyObject::class(
            CompactString::from("_io.BufferedRandom"),
            vec![],
            IndexMap::new(),
        );
        Ok(PyObject::instance_with_attrs(class, attrs))
    }
    #[cfg(not(unix))]
    {
        let _ = (path_str, is_binary, delete);
        Err(PyException::not_implemented_error(
            "NamedTemporaryFile not available on this platform",
        ))
    }
}

pub fn create_tempfile_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_name(prefix: &str, suffix: &str) -> String {
        // Use counter + process ID + random bits to generate unique names
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let rand_bits: u64 = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            n.hash(&mut h);
            pid.hash(&mut h);
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut h);
            h.finish()
        };
        format!(
            "{}{}{}{}{}{}",
            std::env::temp_dir().to_string_lossy(),
            std::path::MAIN_SEPARATOR,
            prefix,
            rand_bits,
            n,
            suffix
        )
    }

    make_module(
        "tempfile",
        vec![
            (
                "gettempdir",
                make_builtin(|_| {
                    Ok(PyObject::str_val(CompactString::from(
                        std::env::temp_dir().to_string_lossy().to_string(),
                    )))
                }),
            ),
            (
                "mkdtemp",
                make_builtin(|args| {
                    let mut suffix = String::new();
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("suffix")))
                            {
                                suffix = v.py_to_string();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    let dir = temp_name(&prefix, &suffix);
                    std::fs::create_dir(&dir)
                        .map_err(|e| PyException::runtime_error(format!("mkdtemp: {}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(dir)))
                }),
            ),
            (
                "mkstemp",
                make_builtin(|args| {
                    let mut suffix = String::new();
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("suffix")))
                            {
                                suffix = v.py_to_string();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    let path = temp_name(&prefix, &suffix);
                    // Open with read+write (O_RDWR | O_CREAT | O_EXCL) like CPython
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|e| PyException::runtime_error(format!("mkstemp: {}", e)))?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::IntoRawFd;
                        let fd = file.into_raw_fd();
                        Ok(PyObject::tuple(vec![
                            PyObject::int(fd as i64),
                            PyObject::str_val(CompactString::from(path)),
                        ]))
                    }
                    #[cfg(not(unix))]
                    {
                        drop(file);
                        Ok(PyObject::tuple(vec![
                            PyObject::int(0),
                            PyObject::str_val(CompactString::from(path)),
                        ]))
                    }
                }),
            ),
            (
                "mktemp",
                make_builtin(|args| {
                    let mut suffix = String::new();
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("suffix")))
                            {
                                suffix = v.py_to_string();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(temp_name(
                        &prefix, &suffix,
                    ))))
                }),
            ),
            ("NamedTemporaryFile", make_builtin(named_temporary_file)),
            ("TemporaryFile", make_builtin(named_temporary_file)),
            ("SpooledTemporaryFile", make_builtin(named_temporary_file)),
            (
                "_TemporaryFileWrapper",
                PyObject::class(CompactString::from("_TemporaryFileWrapper"), vec![], {
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__init__"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                    ns
                }),
            ),
            (
                "TemporaryDirectory",
                make_builtin(|args| {
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    let dir = temp_name(&prefix, "");
                    std::fs::create_dir_all(&dir).map_err(|e| {
                        PyException::runtime_error(format!("TemporaryDirectory: {}", e))
                    })?;

                    let cls = PyObject::class(
                        CompactString::from("TemporaryDirectory"),
                        vec![],
                        IndexMap::new(),
                    );
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("name"),
                        PyObject::str_val(CompactString::from(&dir)),
                    );

                    let dir_enter = dir.clone();
                    attrs.insert(
                        CompactString::from("__enter__"),
                        PyObject::native_closure("TemporaryDirectory.__enter__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(dir_enter.as_str())))
                        }),
                    );
                    let dir_exit = dir.clone();
                    attrs.insert(
                        CompactString::from("__exit__"),
                        PyObject::native_closure("TemporaryDirectory.__exit__", move |_| {
                            let _ = std::fs::remove_dir_all(&dir_exit);
                            Ok(PyObject::bool_val(false))
                        }),
                    );
                    let dir_cleanup = dir;
                    attrs.insert(
                        CompactString::from("cleanup"),
                        PyObject::native_closure("TemporaryDirectory.cleanup", move |_| {
                            let _ = std::fs::remove_dir_all(&dir_cleanup);
                            Ok(PyObject::none())
                        }),
                    );
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }),
            ),
        ],
    )
}
