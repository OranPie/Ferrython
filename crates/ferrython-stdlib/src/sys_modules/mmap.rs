use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

// ── mmap module ──

pub fn create_mmap_module() -> PyObjectRef {
    // mmap.mmap(fileno, length, ...) → mmap object
    // Simplified: backed by Vec<u8> (not real file-backed mapping)
    let mmap_fn = make_builtin(|args: &[PyObjectRef]| {
        let fileno = if !args.is_empty() {
            args[0].to_int().unwrap_or(-1)
        } else {
            -1
        };
        let length = if args.len() > 1 {
            args[1].to_int().unwrap_or(0) as usize
        } else {
            0
        };

        // If fileno >= 0, try to read the file contents
        let initial_data: Vec<u8> = if fileno >= 0 {
            #[cfg(unix)]
            {
                use std::os::unix::io::FromRawFd;
                // Dup the fd so we don't close the caller's fd
                let dup_fd = unsafe { libc::dup(fileno as i32) };
                if dup_fd >= 0 {
                    let mut file = unsafe { std::fs::File::from_raw_fd(dup_fd) };
                    use std::io::Read;
                    let mut buf = Vec::new();
                    let _ = file.read_to_end(&mut buf);
                    if length > 0 {
                        buf.resize(length, 0);
                    }
                    buf
                } else {
                    vec![0u8; length]
                }
            }
            #[cfg(not(unix))]
            {
                vec![0u8; length]
            }
        } else {
            // Anonymous mapping
            vec![0u8; length]
        };

        let data: Rc<PyCell<Vec<u8>>> = Rc::new(PyCell::new(initial_data));
        let pos: Rc<PyCell<usize>> = Rc::new(PyCell::new(0));
        let closed: Rc<PyCell<bool>> = Rc::new(PyCell::new(false));
        let cls = PyObject::class(CompactString::from("mmap"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("closed"), PyObject::bool_val(false));

            // read(n=-1)
            let d2 = data.clone();
            let p2 = pos.clone();
            w.insert(
                CompactString::from("read"),
                PyObject::native_closure("read", move |args| {
                    let n = if !args.is_empty() {
                        args[0].to_int().unwrap_or(-1)
                    } else {
                        -1
                    };
                    let mut p = p2.write();
                    let d = d2.read();
                    let start = *p;
                    let end = if n < 0 {
                        d.len()
                    } else {
                        std::cmp::min(start + n as usize, d.len())
                    };
                    let slice = d[start..end].to_vec();
                    *p = end;
                    Ok(PyObject::bytes(slice))
                }),
            );

            // read_byte()
            let d_rb = data.clone();
            let p_rb = pos.clone();
            w.insert(
                CompactString::from("read_byte"),
                PyObject::native_closure("read_byte", move |_args| {
                    let mut p = p_rb.write();
                    let d = d_rb.read();
                    if *p >= d.len() {
                        return Err(PyException::value_error("read byte out of range"));
                    }
                    let byte = d[*p];
                    *p += 1;
                    Ok(PyObject::int(byte as i64))
                }),
            );

            // write(data)
            let d3 = data.clone();
            let p3 = pos.clone();
            w.insert(
                CompactString::from("write"),
                PyObject::native_closure("write", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("write requires bytes"));
                    }
                    let bytes = match &args[0].payload {
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        _ => return Err(PyException::type_error("write requires bytes argument")),
                    };
                    let mut d = d3.write();
                    let mut p = p3.write();
                    let start = *p;
                    for (i, &byte) in bytes.iter().enumerate() {
                        let idx = start + i;
                        if idx < d.len() {
                            d[idx] = byte;
                        } else {
                            d.push(byte);
                        }
                    }
                    *p = start + bytes.len();
                    Ok(PyObject::int(bytes.len() as i64))
                }),
            );

            // write_byte(byte)
            let d_wb = data.clone();
            let p_wb = pos.clone();
            w.insert(
                CompactString::from("write_byte"),
                PyObject::native_closure("write_byte", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("write_byte requires an integer"));
                    }
                    let byte = args[0].to_int()? as u8;
                    let mut d = d_wb.write();
                    let mut p = p_wb.write();
                    if *p < d.len() {
                        d[*p] = byte;
                    } else {
                        d.push(byte);
                    }
                    *p += 1;
                    Ok(PyObject::none())
                }),
            );

            // seek(pos, whence=0)
            let d_seek = data.clone();
            let p4 = pos.clone();
            w.insert(
                CompactString::from("seek"),
                PyObject::native_closure("seek", move |args| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    let offset = args[0].to_int().unwrap_or(0);
                    let whence = if args.len() > 1 {
                        args[1].to_int().unwrap_or(0)
                    } else {
                        0
                    };
                    let mut p = p4.write();
                    let len = d_seek.read().len() as i64;
                    let new_pos = match whence {
                        0 => offset,             // SEEK_SET
                        1 => *p as i64 + offset, // SEEK_CUR
                        2 => len + offset,       // SEEK_END
                        _ => return Err(PyException::value_error("invalid whence")),
                    };
                    *p = new_pos.max(0) as usize;
                    Ok(PyObject::none())
                }),
            );

            // tell()
            let p5 = pos.clone();
            w.insert(
                CompactString::from("tell"),
                PyObject::native_closure("tell", move |_args| Ok(PyObject::int(*p5.read() as i64))),
            );

            // size()
            let d6 = data.clone();
            w.insert(
                CompactString::from("size"),
                PyObject::native_closure("size", move |_args| {
                    Ok(PyObject::int(d6.read().len() as i64))
                }),
            );

            // find(sub, start=0, end=len)
            let d_find = data.clone();
            w.insert(
                CompactString::from("find"),
                PyObject::native_closure("find", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("find requires bytes argument"));
                    }
                    let sub = match &args[0].payload {
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        PyObjectPayload::Int(n) => vec![n.to_i64().unwrap_or(0) as u8],
                        _ => return Err(PyException::type_error("expected bytes")),
                    };
                    let d = d_find.read();
                    let start = if args.len() > 1 {
                        args[1].to_int().unwrap_or(0) as usize
                    } else {
                        0
                    };
                    let end = if args.len() > 2 {
                        args[2].to_int().unwrap_or(d.len() as i64) as usize
                    } else {
                        d.len()
                    };
                    let end = end.min(d.len());
                    if start >= end || sub.is_empty() {
                        return Ok(PyObject::int(if sub.is_empty() && start <= end {
                            start as i64
                        } else {
                            -1
                        }));
                    }
                    let haystack = &d[start..end];
                    for i in 0..=(haystack.len().saturating_sub(sub.len())) {
                        if haystack[i..].starts_with(&sub) {
                            return Ok(PyObject::int((start + i) as i64));
                        }
                    }
                    Ok(PyObject::int(-1))
                }),
            );

            // rfind(sub, start=0, end=len)
            let d_rfind = data.clone();
            w.insert(
                CompactString::from("rfind"),
                PyObject::native_closure("rfind", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("rfind requires bytes argument"));
                    }
                    let sub = match &args[0].payload {
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        PyObjectPayload::Int(n) => vec![n.to_i64().unwrap_or(0) as u8],
                        _ => return Err(PyException::type_error("expected bytes")),
                    };
                    let d = d_rfind.read();
                    let start = if args.len() > 1 {
                        args[1].to_int().unwrap_or(0) as usize
                    } else {
                        0
                    };
                    let end = if args.len() > 2 {
                        args[2].to_int().unwrap_or(d.len() as i64) as usize
                    } else {
                        d.len()
                    };
                    let end = end.min(d.len());
                    if start >= end || sub.is_empty() {
                        return Ok(PyObject::int(if sub.is_empty() && start <= end {
                            end as i64
                        } else {
                            -1
                        }));
                    }
                    let haystack = &d[start..end];
                    for i in (0..=(haystack.len().saturating_sub(sub.len()))).rev() {
                        if haystack[i..].starts_with(&sub) {
                            return Ok(PyObject::int((start + i) as i64));
                        }
                    }
                    Ok(PyObject::int(-1))
                }),
            );

            // readline()
            let d_rl = data.clone();
            let p_rl = pos.clone();
            w.insert(
                CompactString::from("readline"),
                PyObject::native_closure("readline", move |_args| {
                    let mut p = p_rl.write();
                    let d = d_rl.read();
                    if *p >= d.len() {
                        return Ok(PyObject::bytes(vec![]));
                    }
                    let start = *p;
                    let mut end = start;
                    while end < d.len() && d[end] != b'\n' {
                        end += 1;
                    }
                    if end < d.len() {
                        end += 1;
                    } // include the newline
                    let line = d[start..end].to_vec();
                    *p = end;
                    Ok(PyObject::bytes(line))
                }),
            );

            // resize(newsize)
            let d_resize = data.clone();
            w.insert(
                CompactString::from("resize"),
                PyObject::native_closure("resize", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("resize requires length"));
                    }
                    let new_size = args[0].to_int()? as usize;
                    d_resize.write().resize(new_size, 0);
                    Ok(PyObject::none())
                }),
            );

            // flush(offset=0, size=len)
            w.insert(
                CompactString::from("flush"),
                make_builtin(|_args: &[PyObjectRef]| {
                    // For Vec-backed mmap, flush is a no-op
                    Ok(PyObject::none())
                }),
            );

            // move(dest, src, count)
            let d_move = data.clone();
            w.insert(
                CompactString::from("move"),
                PyObject::native_closure("move", move |args| {
                    if args.len() < 3 {
                        return Err(PyException::type_error("move requires dest, src, count"));
                    }
                    let dest = args[0].to_int()? as usize;
                    let src = args[1].to_int()? as usize;
                    let count = args[2].to_int()? as usize;
                    let mut d = d_move.write();
                    let len = d.len();
                    if src + count > len || dest + count > len {
                        return Err(PyException::value_error(
                            "source or destination out of range",
                        ));
                    }
                    // Use copy_within for safe overlapping moves
                    d.copy_within(src..src + count, dest);
                    Ok(PyObject::none())
                }),
            );

            // close()
            let c_close = closed.clone();
            w.insert(
                CompactString::from("close"),
                PyObject::native_closure("close", move |_args| {
                    *c_close.write() = true;
                    Ok(PyObject::none())
                }),
            );

            // __len__
            let d7 = data.clone();
            w.insert(
                CompactString::from("__len__"),
                PyObject::native_closure("__len__", move |_args| {
                    Ok(PyObject::int(d7.read().len() as i64))
                }),
            );

            // __getitem__ (indexing and slicing)
            let d8 = data.clone();
            w.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure("__getitem__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::index_error("mmap index out of range"));
                    }
                    let d = d8.read();
                    let len = d.len() as i64;
                    // Check for slice (Tuple with start/stop/step from VM slice dispatch)
                    if let PyObjectPayload::Slice(sd) = &args[0].payload {
                        let s = sd.start.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
                        let e = sd.stop.as_ref().and_then(|v| v.as_int()).unwrap_or(len);
                        let s = if s < 0 { (len + s).max(0) } else { s.min(len) } as usize;
                        let e = if e < 0 { (len + e).max(0) } else { e.min(len) } as usize;
                        let result = if s < e { d[s..e].to_vec() } else { vec![] };
                        return Ok(PyObject::bytes(result));
                    }
                    let idx = args[0].to_int().unwrap_or(0);
                    let resolved = if idx < 0 { len + idx } else { idx };
                    if resolved < 0 || resolved >= len {
                        return Err(PyException::index_error("mmap index out of range"));
                    }
                    Ok(PyObject::int(d[resolved as usize] as i64))
                }),
            );

            // __setitem__ (indexing)
            let d_si = data.clone();
            w.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure("__setitem__", move |args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "__setitem__ requires index and value",
                        ));
                    }
                    let idx = args[0].to_int().unwrap_or(0);
                    let val = args[1].to_int()? as u8;
                    let mut d = d_si.write();
                    let len = d.len() as i64;
                    let resolved = if idx < 0 { len + idx } else { idx };
                    if resolved < 0 || resolved >= len {
                        return Err(PyException::index_error(
                            "mmap assignment index out of range",
                        ));
                    }
                    d[resolved as usize] = val;
                    Ok(PyObject::none())
                }),
            );

            // __enter__ / __exit__ for context manager
            let inst_ref = inst.clone();
            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_| Ok(inst_ref.clone())),
            );
            w.insert(
                CompactString::from("__exit__"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
            );

            // __repr__
            let d_repr = data.clone();
            w.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("__repr__", move |_args| {
                    let len = d_repr.read().len();
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "<mmap.mmap object, length={}>",
                        len
                    ))))
                }),
            );
        }
        Ok(inst)
    });

    make_module(
        "mmap",
        vec![
            ("mmap", mmap_fn),
            ("ACCESS_READ", PyObject::int(1)),
            ("ACCESS_WRITE", PyObject::int(2)),
            ("ACCESS_COPY", PyObject::int(3)),
            ("ACCESS_DEFAULT", PyObject::int(0)),
            (
                "PAGESIZE",
                PyObject::int({
                    #[cfg(unix)]
                    {
                        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 }
                    }
                    #[cfg(not(unix))]
                    {
                        4096i64
                    }
                }),
            ),
            (
                "ALLOCATIONGRANULARITY",
                PyObject::int({
                    #[cfg(unix)]
                    {
                        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 }
                    }
                    #[cfg(not(unix))]
                    {
                        65536i64
                    }
                }),
            ),
            ("MAP_SHARED", PyObject::int(1)),
            ("MAP_PRIVATE", PyObject::int(2)),
            ("PROT_READ", PyObject::int(1)),
            ("PROT_WRITE", PyObject::int(2)),
            ("PROT_EXEC", PyObject::int(4)),
        ],
    )
}

// ── resource module (unix) ──
