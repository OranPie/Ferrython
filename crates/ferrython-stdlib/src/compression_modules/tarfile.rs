//! tarfile module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

// ══════════════════════════════════════════════════════════════════════
//  tarfile module
// ══════════════════════════════════════════════════════════════════════

pub fn create_tarfile_module() -> PyObjectRef {
    make_module(
        "tarfile",
        vec![
            ("open", make_builtin(tarfile_open)),
            ("TarFile", make_builtin(tarfile_open)),
            ("TarInfo", make_builtin(tarinfo_constructor)),
            (
                "is_tarfile",
                make_builtin(|args: &[PyObjectRef]| {
                    check_args("tarfile.is_tarfile", args, 1)?;
                    let path = args[0].py_to_string();
                    // Try to open and read magic bytes
                    match std::fs::File::open(&path) {
                        Ok(mut f) => {
                            let mut buf = [0u8; 263];
                            use std::io::Read;
                            let n = f.read(&mut buf).unwrap_or(0);
                            // tar magic at offset 257: "ustar"
                            Ok(PyObject::bool_val(n >= 262 && &buf[257..262] == b"ustar"))
                        }
                        Err(_) => Ok(PyObject::bool_val(false)),
                    }
                }),
            ),
            ("ENCODING", PyObject::str_val(CompactString::from("utf-8"))),
            ("DEFAULT_FORMAT", PyObject::int(1)), // GNU_FORMAT
            ("USTAR_FORMAT", PyObject::int(0)),
            ("GNU_FORMAT", PyObject::int(1)),
            ("PAX_FORMAT", PyObject::int(2)),
        ],
    )
}

struct TarEntry {
    name: String,
    data: Vec<u8>,
    size: u64,
    is_dir: bool,
}

struct TarInner {
    filepath: String,
    mode: String,
    entries: Vec<TarEntry>,
    closed: bool,
    /// The original Python BytesIO object (for writing tar data back on close)
    fileobj: Option<PyObjectRef>,
}

fn build_tarinfo(name: &str, size: u64, is_dir: bool) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(
        CompactString::from("name"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(CompactString::from("size"), PyObject::int(size as i64));
    attrs.insert(CompactString::from("mtime"), PyObject::int(0));
    attrs.insert(CompactString::from("mode"), PyObject::int(0o644));
    attrs.insert(CompactString::from("uid"), PyObject::int(0));
    attrs.insert(CompactString::from("gid"), PyObject::int(0));
    attrs.insert(
        CompactString::from("uname"),
        PyObject::str_val(CompactString::from("")),
    );
    attrs.insert(
        CompactString::from("gname"),
        PyObject::str_val(CompactString::from("")),
    );
    attrs.insert(
        CompactString::from("type"),
        if is_dir {
            PyObject::str_val(CompactString::from("5")) // DIRTYPE
        } else {
            PyObject::str_val(CompactString::from("0")) // REGTYPE
        },
    );
    attrs.insert(
        CompactString::from("isdir"),
        PyObject::native_closure("isdir", {
            let d = is_dir;
            move |_args| Ok(PyObject::bool_val(d))
        }),
    );
    attrs.insert(
        CompactString::from("isfile"),
        PyObject::native_closure("isfile", {
            let d = is_dir;
            move |_args| Ok(PyObject::bool_val(!d))
        }),
    );

    let cls = PyObject::class(CompactString::from("TarInfo"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn build_tarfile_object(inner: Arc<Mutex<TarInner>>) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__tarfile__"), PyObject::bool_val(true));

    // getnames()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("getnames"),
            PyObject::native_closure("getnames", move |_args| {
                let g = st.lock().unwrap();
                let names: Vec<PyObjectRef> = g
                    .entries
                    .iter()
                    .map(|e| PyObject::str_val(CompactString::from(e.name.as_str())))
                    .collect();
                Ok(PyObject::list(names))
            }),
        );
    }

    // getmembers()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("getmembers"),
            PyObject::native_closure("getmembers", move |_args| {
                let g = st.lock().unwrap();
                let members: Vec<PyObjectRef> = g
                    .entries
                    .iter()
                    .map(|e| build_tarinfo(&e.name, e.size, e.is_dir))
                    .collect();
                Ok(PyObject::list(members))
            }),
        );
    }

    // getmember(name) → TarInfo
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("getmember"),
            PyObject::native_closure("getmember", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "getmember() requires name argument",
                    ));
                }
                let name = args[0].py_to_string();
                let g = st.lock().unwrap();
                for entry in &g.entries {
                    if entry.name == name {
                        return Ok(build_tarinfo(&entry.name, entry.size, entry.is_dir));
                    }
                }
                Err(PyException::key_error(&format!("KeyError: '{name}'")))
            }),
        );
    }

    // extractall(path='.')
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("extractall"),
            PyObject::native_closure("extractall", move |args| {
                let dest = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    ".".to_string()
                };
                let g = st.lock().unwrap();
                for entry in &g.entries {
                    let target = format!("{}/{}", dest, entry.name);
                    if entry.is_dir {
                        let _ = std::fs::create_dir_all(&target);
                    } else {
                        if let Some(parent) = std::path::Path::new(&target).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::write(&target, &entry.data).map_err(|e| {
                            PyException::runtime_error(&format!("tarfile.extractall: {e}"))
                        })?;
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // extractfile(member)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("extractfile"),
            PyObject::native_closure("extractfile", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("extractfile() requires member arg"));
                }
                let name = args[0].py_to_string();
                let g = st.lock().unwrap();
                for entry in &g.entries {
                    if entry.name == name {
                        // Return a BytesIO-like object
                        let data = entry.data.clone();
                        let pos: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
                        let mut file_attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                        {
                            let d = data.clone();
                            let p = pos.clone();
                            file_attrs.insert(
                                CompactString::from("read"),
                                PyObject::native_closure("read", move |args| {
                                    let mut cur = p.lock().unwrap();
                                    let n = if !args.is_empty() {
                                        args[0].as_int().unwrap_or(-1)
                                    } else {
                                        -1
                                    };
                                    let remaining = &d[*cur..];
                                    let chunk = if n < 0 {
                                        remaining.to_vec()
                                    } else {
                                        remaining[..remaining.len().min(n as usize)].to_vec()
                                    };
                                    *cur += chunk.len();
                                    Ok(PyObject::bytes(chunk))
                                }),
                            );
                        }
                        let cls = PyObject::class(
                            CompactString::from("ExFileObject"),
                            vec![],
                            IndexMap::new(),
                        );
                        return Ok(PyObject::instance_with_attrs(cls, file_attrs));
                    }
                }
                Err(PyException::key_error(&format!("KeyError: '{name}'")))
            }),
        );
    }

    // add(name, arcname=None)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("add"),
            PyObject::native_closure("add", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("add() requires name argument"));
                }
                let filepath = args[0].py_to_string();
                // Parse arcname from positional arg[1] or kwargs dict
                let arcname = if args.len() > 1 {
                    if let PyObjectPayload::Dict(kw) = &args[args.len() - 1].payload {
                        // Last arg is kwargs dict
                        let r = kw.read();
                        r.get(&HashableKey::str_key(CompactString::from("arcname")))
                            .map(|v| v.py_to_string())
                            .unwrap_or_else(|| filepath.clone())
                    } else if !matches!(&args[1].payload, PyObjectPayload::None) {
                        args[1].py_to_string()
                    } else {
                        filepath.clone()
                    }
                } else {
                    filepath.clone()
                };
                let mut g = st.lock().unwrap();
                let path = std::path::Path::new(&filepath);
                if path.is_dir() {
                    g.entries.push(TarEntry {
                        name: arcname,
                        data: vec![],
                        size: 0,
                        is_dir: true,
                    });
                } else {
                    let data = std::fs::read(&filepath)
                        .map_err(|e| PyException::runtime_error(&format!("tarfile.add: {e}")))?;
                    let size = data.len() as u64;
                    g.entries.push(TarEntry {
                        name: arcname,
                        data,
                        size,
                        is_dir: false,
                    });
                }
                Ok(PyObject::none())
            }),
        );
    }

    // addfile(tarinfo, fileobj=None)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("addfile"),
            PyObject::native_closure("addfile", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("addfile() requires tarinfo"));
                }
                let name = args[0]
                    .get_attr("name")
                    .map(|n| n.py_to_string())
                    .unwrap_or_default();
                let data = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    // Try reading data from fileobj
                    if let PyObjectPayload::Bytes(b) = &args[1].payload {
                        (**b).clone()
                    } else {
                        extract_bytes_from_fileobj(&args[1]).unwrap_or_default()
                    }
                } else {
                    vec![]
                };
                let size = data.len() as u64;
                let mut g = st.lock().unwrap();
                g.entries.push(TarEntry {
                    name,
                    data,
                    size,
                    is_dir: false,
                });
                Ok(PyObject::none())
            }),
        );
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                let mut g = st.lock().unwrap();
                if g.closed {
                    return Ok(PyObject::none());
                }
                g.closed = true;
                if g.mode.contains('w') {
                    write_tar_to_disk(&g)?;
                }
                Ok(PyObject::none())
            }),
        );
    }

    // __enter__ / __exit__
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", move |_args| {
                Ok(build_tarfile_object(st.clone()))
            }),
        );
    }
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let mut g = st.lock().unwrap();
                if !g.closed {
                    g.closed = true;
                    if g.mode.contains('w') {
                        let _ = write_tar_to_disk(&g);
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // name attribute
    {
        let path = inner.lock().unwrap().filepath.clone();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(path.as_str())),
        );
    }

    let cls = PyObject::class(CompactString::from("TarFile"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn write_tar_to_disk(inner: &TarInner) -> PyResult<()> {
    let build_tar_bytes = |entries: &[TarEntry]| -> PyResult<Vec<u8>> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut tar_builder = tar::Builder::new(cursor);
        for entry in entries {
            if entry.is_dir {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_cksum();
                tar_builder
                    .append_data(&mut header, &entry.name, &[][..])
                    .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
            } else {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(entry.data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar_builder
                    .append_data(&mut header, &entry.name, &entry.data[..])
                    .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
            }
        }
        let cursor = tar_builder
            .into_inner()
            .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
        Ok(cursor.into_inner())
    };

    // Write to fileobj if present
    if let Some(ref fobj) = inner.fileobj {
        let data = build_tar_bytes(&inner.entries)?;
        // Write data back to the BytesIO by calling its write method
        if let Some(write_fn) = fobj.get_attr("write") {
            match &write_fn.payload {
                PyObjectPayload::NativeFunction(nf) => {
                    (nf.func)(&[PyObject::bytes(data)])?;
                }
                PyObjectPayload::NativeClosure(nc) => {
                    (nc.func)(&[PyObject::bytes(data)])?;
                }
                _ => {}
            }
        }
        // Seek back to beginning
        if let Some(seek_fn) = fobj.get_attr("seek") {
            match &seek_fn.payload {
                PyObjectPayload::NativeFunction(nf) => {
                    let _ = (nf.func)(&[PyObject::int(0)]);
                }
                PyObjectPayload::NativeClosure(nc) => {
                    let _ = (nc.func)(&[PyObject::int(0)]);
                }
                _ => {}
            }
        }
        return Ok(());
    }

    let filepath = &inner.filepath;
    let file = std::fs::File::create(filepath)
        .map_err(|e| PyException::runtime_error(&format!("tarfile.close: {e}")))?;

    let writer: Box<dyn Write> = if filepath.ends_with(".gz") || filepath.ends_with(".tgz") {
        Box::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::default(),
        ))
    } else if filepath.ends_with(".bz2") {
        Box::new(bzip2::write::BzEncoder::new(
            file,
            bzip2::Compression::default(),
        ))
    } else if filepath.ends_with(".xz") {
        Box::new(xz2::write::XzEncoder::new(file, 6))
    } else {
        Box::new(file)
    };

    let data = build_tar_bytes(&inner.entries)?;
    let mut writer = writer;
    writer
        .write_all(&data)
        .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
    Ok(())
}

fn tarfile_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Parse kwargs from last arg if it's a Dict
    let kwargs = args.last().and_then(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            Some(kw.clone())
        } else {
            None
        }
    });
    let fileobj = kwargs.as_ref().and_then(|kw| {
        let r = kw.read();
        r.get(&HashableKey::str_key(CompactString::from("fileobj")))
            .cloned()
    });
    let mode_kwarg = kwargs.as_ref().and_then(|kw| {
        let r = kw.read();
        r.get(&HashableKey::str_key(CompactString::from("mode")))
            .map(|v| v.py_to_string())
    });

    // Determine mode: positional arg[1] > kwarg > default "r"
    let pos_mode = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::Dict(_)) {
        Some(args[1].py_to_string())
    } else {
        None
    };
    let mode = pos_mode.or(mode_kwarg).unwrap_or_else(|| "r".to_string());

    // fileobj= path: read bytes from BytesIO-like object
    if let Some(fobj) = fileobj {
        let buf_data = extract_bytes_from_fileobj(&fobj)?;
        let entries = if mode.starts_with('r') || mode.contains(":r") {
            read_tar_entries_from_bytes(&buf_data)?
        } else {
            Vec::new()
        };
        return Ok(build_tarfile_object(Arc::new(Mutex::new(TarInner {
            filepath: String::new(),
            mode,
            entries,
            closed: false,
            fileobj: Some(fobj),
        }))));
    }

    if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
        return Err(PyException::type_error(
            "tarfile.open() missing required argument: 'name'",
        ));
    }
    let filepath = args[0].py_to_string();

    let entries = if mode.starts_with('r') || mode.contains(":r") {
        read_tar_entries(&filepath)?
    } else {
        Vec::new()
    };

    Ok(build_tarfile_object(Arc::new(Mutex::new(TarInner {
        filepath,
        mode,
        entries,
        closed: false,
        fileobj: None,
    }))))
}

/// Extract raw bytes from a BytesIO or similar object
fn extract_bytes_from_fileobj(fobj: &PyObjectRef) -> PyResult<Vec<u8>> {
    // Try direct bytes payload
    if let PyObjectPayload::Bytes(b) = &fobj.payload {
        return Ok((**b).clone());
    }
    // Try BytesIO: look for _buffer attribute
    if let Some(buf_attr) = fobj.get_attr("_buffer") {
        if let PyObjectPayload::Bytes(b) = &buf_attr.payload {
            return Ok((**b).clone());
        }
    }
    // Try getvalue() method
    if let Some(getvalue) = fobj.get_attr("getvalue") {
        // Can't call Python method from native code, but NativeFunction is OK
        if let PyObjectPayload::NativeFunction(nf) = &getvalue.payload {
            let result = (nf.func)(&[])?;
            if let PyObjectPayload::Bytes(b) = &result.payload {
                return Ok((**b).clone());
            }
        }
        if let PyObjectPayload::NativeClosure(nc) = &getvalue.payload {
            let result = (nc.func)(&[])?;
            if let PyObjectPayload::Bytes(b) = &result.payload {
                return Ok((**b).clone());
            }
        }
    }
    Err(PyException::type_error(
        "fileobj must be a BytesIO or bytes-like object",
    ))
}

fn read_tar_entries_from_bytes(data: &[u8]) -> PyResult<Vec<TarEntry>> {
    let reader = std::io::Cursor::new(data);
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    for entry_result in archive
        .entries()
        .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?
    {
        let mut entry =
            entry_result.map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
        let name = entry
            .path()
            .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?
            .to_string_lossy()
            .to_string();
        let is_dir = entry.header().entry_type().is_dir();
        let size = entry.size();
        let mut edata = Vec::new();
        if !is_dir {
            entry
                .read_to_end(&mut edata)
                .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
        }
        entries.push(TarEntry {
            name,
            data: edata,
            size,
            is_dir,
        });
    }
    Ok(entries)
}

fn read_tar_entries(filepath: &str) -> PyResult<Vec<TarEntry>> {
    let file = std::fs::File::open(filepath)
        .map_err(|e| PyException::runtime_error(&format!("tarfile.open: {e}")))?;

    let reader: Box<dyn Read> = if filepath.ends_with(".gz") || filepath.ends_with(".tgz") {
        Box::new(flate2::read::GzDecoder::new(file))
    } else if filepath.ends_with(".bz2") {
        Box::new(bzip2::read::BzDecoder::new(file))
    } else if filepath.ends_with(".xz") {
        Box::new(xz2::read::XzDecoder::new(file))
    } else {
        Box::new(file)
    };

    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    for entry_result in archive
        .entries()
        .map_err(|e| PyException::runtime_error(&format!("tarfile.open: {e}")))?
    {
        let mut entry =
            entry_result.map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
        let name = entry
            .path()
            .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?
            .to_string_lossy()
            .to_string();
        let is_dir = entry.header().entry_type().is_dir();
        let size = entry.size();
        let mut data = Vec::new();
        if !is_dir {
            entry
                .read_to_end(&mut data)
                .map_err(|e| PyException::runtime_error(&format!("tarfile: {e}")))?;
        }
        entries.push(TarEntry {
            name,
            data,
            size,
            is_dir,
        });
    }
    Ok(entries)
}

fn tarinfo_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Handle TarInfo(name=...) with kwargs or positional
    let (name, size) = if !args.is_empty() {
        if let PyObjectPayload::Dict(kw) = &args[0].payload {
            let r = kw.read();
            let n = r
                .get(&HashableKey::str_key(CompactString::from("name")))
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            let s = r
                .get(&HashableKey::str_key(CompactString::from("size")))
                .and_then(|v| v.as_int())
                .unwrap_or(0) as u64;
            (n, s)
        } else {
            let n = args[0].py_to_string();
            let s = if args.len() > 1 {
                if let PyObjectPayload::Dict(kw) = &args[1].payload {
                    let r = kw.read();
                    r.get(&HashableKey::str_key(CompactString::from("size")))
                        .and_then(|v| v.as_int())
                        .unwrap_or(0) as u64
                } else {
                    args[1].as_int().unwrap_or(0) as u64
                }
            } else {
                0
            };
            (n, s)
        }
    } else {
        (String::new(), 0)
    };
    Ok(build_tarinfo(&name, size, false))
}
