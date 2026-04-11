//! Compression stdlib modules: gzip and zipfile
//! Uses flate2 for gzip and the zip crate for zipfile operations.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

// ── helpers ──

fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

// ══════════════════════════════════════════════════════════════════════
//  gzip module
// ══════════════════════════════════════════════════════════════════════

pub fn create_gzip_module() -> PyObjectRef {
    make_module("gzip", vec![
        ("compress", make_builtin(gzip_compress)),
        ("decompress", make_builtin(gzip_decompress)),
        ("open", make_builtin(gzip_open)),
        ("GzipFile", make_builtin(gzip_file_constructor)),
        ("BadGzipFile", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
    ])
}

fn gzip_compress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "gzip.compress() missing 1 required positional argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;

    let level = if args.len() > 1 {
        args[1].as_int().unwrap_or(9) as u32
    } else {
        9
    };
    let compression = flate2::Compression::new(level.min(9));

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), compression);
    encoder.write_all(&data).map_err(|e| {
        PyException::runtime_error(&format!("gzip.compress: {}", e))
    })?;
    let compressed = encoder.finish().map_err(|e| {
        PyException::runtime_error(&format!("gzip.compress: {}", e))
    })?;

    Ok(PyObject::bytes(compressed))
}

fn gzip_decompress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "gzip.decompress() missing 1 required positional argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;

    let mut decoder = flate2::read::GzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| {
        PyException::runtime_error(&format!("gzip.decompress: {}", e))
    })?;

    Ok(PyObject::bytes(decompressed))
}

/// Internal state for a gzip file opened via gzip.open().
struct GzipFileInner {
    mode: String,
    filepath: String,
    buffer: Vec<u8>,
    closed: bool,
}

fn build_gzip_file_object(inner: Arc<Mutex<GzipFileInner>>) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__gzipfile__"), PyObject::bool_val(true));

    // name attribute
    {
        let g = inner.lock().unwrap();
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(g.filepath.as_str())));
        attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(g.mode.as_str())));
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(g.closed));
    }

    // read(size=-1)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("read"),
            PyObject::native_closure("read", move |args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let size = if !args.is_empty() { args[0].as_int().unwrap_or(-1) } else { -1 };
                if size < 0 || size as usize >= guard.buffer.len() {
                    Ok(PyObject::bytes(guard.buffer.clone()))
                } else {
                    Ok(PyObject::bytes(guard.buffer[..size as usize].to_vec()))
                }
            }));
    }

    // readline()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readline"),
            PyObject::native_closure("readline", move |_args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let pos = guard.buffer.iter().position(|&b| b == b'\n');
                match pos {
                    Some(i) => Ok(PyObject::bytes(guard.buffer[..=i].to_vec())),
                    None => Ok(PyObject::bytes(guard.buffer.clone())),
                }
            }));
    }

    // readlines()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readlines"),
            PyObject::native_closure("readlines", move |_args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let mut lines = Vec::new();
                let mut start = 0;
                for (i, &b) in guard.buffer.iter().enumerate() {
                    if b == b'\n' {
                        lines.push(PyObject::bytes(guard.buffer[start..=i].to_vec()));
                        start = i + 1;
                    }
                }
                if start < guard.buffer.len() {
                    lines.push(PyObject::bytes(guard.buffer[start..].to_vec()));
                }
                Ok(PyObject::list(lines))
            }));
    }

    // write(data)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("write"),
            PyObject::native_closure("write", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("write() requires a data argument"));
                }
                let data = extract_bytes(&args[0])?;
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let len = data.len();
                guard.buffer.extend(data);
                Ok(PyObject::int(len as i64))
            }));
    }

    // flush()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                Ok(PyObject::none())
            }));
    }

    // seek(offset, whence=0)
    {
        attrs.insert(CompactString::from("seek"),
            PyObject::native_closure("seek", move |_args| {
                Err(PyException::runtime_error("seek() not supported on gzip files"))
            }));
    }

    // tell()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("tell"),
            PyObject::native_closure("tell", move |_args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                Ok(PyObject::int(guard.buffer.len() as i64))
            }));
    }

    // seekable()
    attrs.insert(CompactString::from("seekable"), make_builtin(|_| Ok(PyObject::bool_val(false))));

    // readable()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readable"),
            PyObject::native_closure("readable", move |_args| {
                let guard = st.lock().unwrap();
                Ok(PyObject::bool_val(guard.mode.contains('r')))
            }));
    }

    // writable()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("writable"),
            PyObject::native_closure("writable", move |_args| {
                let guard = st.lock().unwrap();
                Ok(PyObject::bool_val(guard.mode.contains('w') || guard.mode.contains('a')))
            }));
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Ok(PyObject::none());
                }
                guard.closed = true;
                if guard.mode.contains('w') {
                    let compression = flate2::Compression::new(9);
                    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), compression);
                    encoder.write_all(&guard.buffer).map_err(|e| {
                        PyException::runtime_error(&format!("gzip close: {}", e))
                    })?;
                    let compressed = encoder.finish().map_err(|e| {
                        PyException::runtime_error(&format!("gzip close: {}", e))
                    })?;
                    std::fs::write(&guard.filepath, &compressed).map_err(|e| {
                        PyException::runtime_error(&format!("gzip close: {}", e))
                    })?;
                }
                Ok(PyObject::none())
            }));
    }

    // __enter__
    {
        attrs.insert(CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", {
                let st = inner.clone();
                move |_args| {
                    Ok(build_gzip_file_object(st.clone()))
                }
            }));
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let mut guard = st.lock().unwrap();
                if !guard.closed {
                    guard.closed = true;
                    if guard.mode.contains('w') {
                        let compression = flate2::Compression::new(9);
                        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), compression);
                        let _ = encoder.write_all(&guard.buffer);
                        if let Ok(compressed) = encoder.finish() {
                            let _ = std::fs::write(&guard.filepath, &compressed);
                        }
                    }
                }
                Ok(PyObject::none())
            }));
    }

    let cls = PyObject::class(CompactString::from("GzipFile"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn gzip_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "gzip.open() missing 1 required positional argument: 'filename'",
        ));
    }
    let filepath = args[0].py_to_string();
    let mode = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "rb".to_string()
    };
    gzip_open_with(&filepath, &mode)
}

/// GzipFile(filename=None, mode='rb', compresslevel=9, fileobj=None)
/// Supports both file path and fileobj (BytesIO) arguments.
fn gzip_file_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut filepath = String::new();
    let mut mode = "rb".to_string();
    let mut fileobj: Option<PyObjectRef> = None;

    // Parse positional and keyword arguments
    for (i, arg) in args.iter().enumerate() {
        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
            let r = kw_map.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("filename"))) {
                if !matches!(&v.payload, PyObjectPayload::None) { filepath = v.py_to_string(); }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("mode"))) {
                mode = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("fileobj"))) {
                if !matches!(&v.payload, PyObjectPayload::None) { fileobj = Some(v.clone()); }
            }
        } else {
            match i {
                0 => if !matches!(&arg.payload, PyObjectPayload::None) { filepath = arg.py_to_string(); }
                1 => mode = arg.py_to_string(),
                _ => {}
            }
        }
    }

    // If fileobj is provided, read/write from it instead of filesystem
    if let Some(fobj) = fileobj {
        let buffer = if mode.contains('r') {
            // Extract bytes from BytesIO-like object via internal buffer attribute
            if let Some(buf_ref) = fobj.get_attr("_buf") {
                extract_bytes(&buf_ref).unwrap_or_default()
            } else if let Some(val_ref) = fobj.get_attr("_value") {
                extract_bytes(&val_ref).unwrap_or_default()
            } else {
                extract_bytes(&fobj).unwrap_or_default()
            }
        } else {
            Vec::new()
        };

        // For read mode, decompress the buffer
        let decompressed = if mode.contains('r') && !buffer.is_empty() {
            let mut decoder = flate2::read::GzDecoder::new(&buffer[..]);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).map_err(|e| {
                PyException::runtime_error(&format!("GzipFile: {}", e))
            })?;
            out
        } else {
            buffer
        };

        let inner = Arc::new(Mutex::new(GzipFileInner {
            mode: mode.clone(),
            filepath: filepath.clone(),
            buffer: decompressed,
            closed: false,
        }));

        return Ok(build_gzip_file_object(inner));
    }

    // File-path based (same as gzip.open)
    if filepath.is_empty() {
        return Err(PyException::type_error("GzipFile requires filename or fileobj"));
    }
    gzip_open_with(&filepath, &mode)
}

fn gzip_open_with(filepath: &str, mode: &str) -> PyResult<PyObjectRef> {
    let buffer = if mode.contains('r') {
        let raw = std::fs::read(filepath).map_err(|e| {
            PyException::runtime_error(&format!("GzipFile: {}", e))
        })?;
        let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|e| {
            PyException::runtime_error(&format!("GzipFile: {}", e))
        })?;
        decompressed
    } else {
        Vec::new()
    };

    let inner = Arc::new(Mutex::new(GzipFileInner {
        mode: mode.to_string(),
        filepath: filepath.to_string(),
        buffer,
        closed: false,
    }));

    Ok(build_gzip_file_object(inner))
}

// ══════════════════════════════════════════════════════════════════════
//  zipfile module
// ══════════════════════════════════════════════════════════════════════

pub fn create_zipfile_module() -> PyObjectRef {
    make_module("zipfile", vec![
        ("ZipFile", make_builtin(zipfile_constructor)),
        ("ZipInfo", make_builtin(zipinfo_constructor)),
        ("ZIP_STORED", PyObject::int(0)),
        ("ZIP_DEFLATED", PyObject::int(8)),
    ])
}

/// Internal state for a zip archive.
struct ZipInner {
    mode: String,
    filepath: String,
    entries: IndexMap<String, Vec<u8>>,
    closed: bool,
}

fn build_zipinfo(filename: &str, size: usize) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__zipinfo__"), PyObject::bool_val(true));
    attrs.insert(CompactString::from("filename"), PyObject::str_val(CompactString::from(filename)));
    attrs.insert(CompactString::from("file_size"), PyObject::int(size as i64));
    attrs.insert(CompactString::from("compress_size"), PyObject::int(size as i64));
    attrs.insert(CompactString::from("compress_type"), PyObject::int(0));
    let cls = PyObject::class(CompactString::from("ZipInfo"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn build_zipfile_object(inner: Arc<Mutex<ZipInner>>) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__zipfile__"), PyObject::bool_val(true));

    // write(filename, arcname=None)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("write"),
            PyObject::native_closure("write", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("write() requires a filename argument"));
                }
                let filename = args[0].py_to_string();
                let arcname = if args.len() > 1 {
                    args[1].py_to_string()
                } else {
                    filename.clone()
                };
                let data = std::fs::read(&filename).map_err(|e| {
                    PyException::runtime_error(&format!("zipfile.write: {}", e))
                })?;
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("zipfile: I/O operation on closed file"));
                }
                guard.entries.insert(arcname, data);
                Ok(PyObject::none())
            }));
    }

    // writestr(arcname, data)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("writestr"),
            PyObject::native_closure("writestr", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("writestr() requires arcname and data"));
                }
                let arcname = args[0].py_to_string();
                let data = extract_bytes(&args[1]).unwrap_or_else(|_| {
                    args[1].py_to_string().into_bytes()
                });
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("zipfile: I/O operation on closed file"));
                }
                guard.entries.insert(arcname, data);
                Ok(PyObject::none())
            }));
    }

    // namelist()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("namelist"),
            PyObject::native_closure("namelist", move |_args| {
                let guard = st.lock().unwrap();
                let names: Vec<PyObjectRef> = guard.entries
                    .keys()
                    .map(|k| PyObject::str_val(CompactString::from(k.as_str())))
                    .collect();
                Ok(PyObject::list(names))
            }));
    }

    // read(name)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("read"),
            PyObject::native_closure("read", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("read() requires a name argument"));
                }
                let name = args[0].py_to_string();
                let guard = st.lock().unwrap();
                match guard.entries.get(&name) {
                    Some(data) => Ok(PyObject::bytes(data.clone())),
                    None => Err(PyException::key_error(&format!(
                        "There is no item named '{}' in the archive", name
                    ))),
                }
            }));
    }

    // infolist()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("infolist"),
            PyObject::native_closure("infolist", move |_args| {
                let guard = st.lock().unwrap();
                let infos: Vec<PyObjectRef> = guard.entries
                    .iter()
                    .map(|(name, data)| build_zipinfo(name, data.len()))
                    .collect();
                Ok(PyObject::list(infos))
            }));
    }

    // getinfo(name)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("getinfo"),
            PyObject::native_closure("getinfo", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("getinfo() requires a name argument"));
                }
                let name = args[0].py_to_string();
                let guard = st.lock().unwrap();
                match guard.entries.get(&name) {
                    Some(data) => Ok(build_zipinfo(&name, data.len())),
                    None => Err(PyException::key_error(&format!(
                        "There is no item named '{}' in the archive", name
                    ))),
                }
            }));
    }

    // extractall(path='.')
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("extractall"),
            PyObject::native_closure("extractall", move |args| {
                let dest = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    ".".to_string()
                };
                let guard = st.lock().unwrap();
                for (name, data) in guard.entries.iter() {
                    let path = std::path::Path::new(&dest).join(name);
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&path, data).map_err(|e| {
                        PyException::runtime_error(&format!("extractall: {}", e))
                    })?;
                }
                Ok(PyObject::none())
            }));
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                zip_close_inner(&st)
            }));
    }

    // __enter__
    {
        attrs.insert(CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", {
                let st = inner.clone();
                move |_args| {
                    Ok(build_zipfile_object(st.clone()))
                }
            }));
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let _ = zip_close_inner(&st);
                Ok(PyObject::none())
            }));
    }

    let cls = PyObject::class(CompactString::from("ZipFile"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

/// Flush and close a zip archive, writing it to disk using the `zip` crate.
fn zip_close_inner(st: &Arc<Mutex<ZipInner>>) -> PyResult<PyObjectRef> {
    let mut guard = st.lock().unwrap();
    if guard.closed {
        return Ok(PyObject::none());
    }
    guard.closed = true;
    if guard.mode.contains('w') {
        let file = std::fs::File::create(&guard.filepath).map_err(|e| {
            PyException::runtime_error(&format!("zipfile.close: {}", e))
        })?;
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in guard.entries.iter() {
            writer.start_file(name, options).map_err(|e| {
                PyException::runtime_error(&format!("zipfile.close: {}", e))
            })?;
            writer.write_all(data).map_err(|e| {
                PyException::runtime_error(&format!("zipfile.close: {}", e))
            })?;
        }
        writer.finish().map_err(|e| {
            PyException::runtime_error(&format!("zipfile.close: {}", e))
        })?;
    }
    Ok(PyObject::none())
}

fn zipfile_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "ZipFile() missing required argument: 'file'",
        ));
    }
    let filepath = args[0].py_to_string();
    let mode = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "r".to_string()
    };

    let entries = if mode.contains('r') {
        let file = std::fs::File::open(&filepath).map_err(|e| {
            PyException::runtime_error(&format!("zipfile: {}", e))
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| {
            PyException::runtime_error(&format!("zipfile: {}", e))
        })?;
        let mut map = IndexMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| {
                PyException::runtime_error(&format!("zipfile: {}", e))
            })?;
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| {
                PyException::runtime_error(&format!("zipfile: {}", e))
            })?;
            map.insert(name, data);
        }
        map
    } else {
        IndexMap::new()
    };

    let inner = Arc::new(Mutex::new(ZipInner {
        mode,
        filepath,
        entries,
        closed: false,
    }));

    Ok(build_zipfile_object(inner))
}

fn zipinfo_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let filename = if !args.is_empty() {
        args[0].py_to_string()
    } else {
        String::new()
    };
    Ok(build_zipinfo(&filename, 0))
}

// ══════════════════════════════════════════════════════════════════════
//  bz2 module
// ══════════════════════════════════════════════════════════════════════

pub fn create_bz2_module() -> PyObjectRef {
    make_module("bz2", vec![
        ("compress", make_builtin(bz2_compress)),
        ("decompress", make_builtin(bz2_decompress)),
        ("open", make_builtin(bz2_open)),
        ("BZ2Compressor", make_builtin(bz2_compressor_ctor)),
        ("BZ2Decompressor", make_builtin(bz2_decompressor_ctor)),
    ])
}

fn bz2_compress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "bz2.compress() missing required argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    let level = if args.len() > 1 {
        args[1].as_int().unwrap_or(9) as u32
    } else {
        9
    };

    let mut encoder = bzip2::write::BzEncoder::new(
        Vec::new(),
        bzip2::Compression::new(level.min(9)),
    );
    encoder.write_all(&data).map_err(|e| {
        PyException::runtime_error(&format!("bz2.compress: {e}"))
    })?;
    let compressed = encoder.finish().map_err(|e| {
        PyException::runtime_error(&format!("bz2.compress: {e}"))
    })?;
    Ok(PyObject::bytes(compressed))
}

fn bz2_decompress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "bz2.decompress() missing required argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    let mut decoder = bzip2::read::BzDecoder::new(&data[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| {
        PyException::runtime_error(&format!("bz2.decompress: {e}"))
    })?;
    Ok(PyObject::bytes(out))
}

struct Bz2FileInner {
    mode: String,
    filepath: String,
    buffer: Vec<u8>,
    closed: bool,
}

fn build_bz2_file(inner: Arc<Mutex<Bz2FileInner>>) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__bz2file__"), PyObject::bool_val(true));

    // name / mode / closed attributes
    {
        let g = inner.lock().unwrap();
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(g.filepath.as_str())));
        attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(g.mode.as_str())));
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(g.closed));
    }

    // read(size=-1)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("read"),
            PyObject::native_closure("read", move |args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                let size = if !args.is_empty() { args[0].as_int().unwrap_or(-1) } else { -1 };
                if size < 0 || size as usize >= g.buffer.len() {
                    Ok(PyObject::bytes(g.buffer.clone()))
                } else {
                    Ok(PyObject::bytes(g.buffer[..size as usize].to_vec()))
                }
            }));
    }

    // readline()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readline"),
            PyObject::native_closure("readline", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                match g.buffer.iter().position(|&b| b == b'\n') {
                    Some(i) => Ok(PyObject::bytes(g.buffer[..=i].to_vec())),
                    None => Ok(PyObject::bytes(g.buffer.clone())),
                }
            }));
    }

    // readlines()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readlines"),
            PyObject::native_closure("readlines", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                let mut lines = Vec::new();
                let mut start = 0;
                for (i, &b) in g.buffer.iter().enumerate() {
                    if b == b'\n' {
                        lines.push(PyObject::bytes(g.buffer[start..=i].to_vec()));
                        start = i + 1;
                    }
                }
                if start < g.buffer.len() {
                    lines.push(PyObject::bytes(g.buffer[start..].to_vec()));
                }
                Ok(PyObject::list(lines))
            }));
    }

    // write(data)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("write"),
            PyObject::native_closure("write", move |args| {
                if args.is_empty() { return Err(PyException::type_error("write() requires data")); }
                let data = extract_bytes(&args[0])?;
                let mut g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                let len = data.len();
                g.buffer.extend(data);
                Ok(PyObject::int(len as i64))
            }));
    }

    // flush()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                Ok(PyObject::none())
            }));
    }

    // tell()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("tell"),
            PyObject::native_closure("tell", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                Ok(PyObject::int(g.buffer.len() as i64))
            }));
    }

    // seek()
    attrs.insert(CompactString::from("seek"),
        PyObject::native_closure("seek", move |_args| {
            Err(PyException::runtime_error("seek() not supported on bz2 files"))
        }));

    // seekable() / readable() / writable()
    attrs.insert(CompactString::from("seekable"), make_builtin(|_| Ok(PyObject::bool_val(false))));
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readable"),
            PyObject::native_closure("readable", move |_args| {
                let g = st.lock().unwrap();
                Ok(PyObject::bool_val(g.mode.contains('r')))
            }));
    }
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("writable"),
            PyObject::native_closure("writable", move |_args| {
                let g = st.lock().unwrap();
                Ok(PyObject::bool_val(g.mode.contains('w') || g.mode.contains('a')))
            }));
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                let mut g = st.lock().unwrap();
                if g.closed { return Ok(PyObject::none()); }
                g.closed = true;
                if g.mode.contains('w') {
                    let mut enc = bzip2::write::BzEncoder::new(
                        Vec::new(), bzip2::Compression::new(9),
                    );
                    enc.write_all(&g.buffer).map_err(|e| {
                        PyException::runtime_error(&format!("bz2 close: {e}"))
                    })?;
                    let compressed = enc.finish().map_err(|e| {
                        PyException::runtime_error(&format!("bz2 close: {e}"))
                    })?;
                    std::fs::write(&g.filepath, &compressed).map_err(|e| {
                        PyException::runtime_error(&format!("bz2 close: {e}"))
                    })?;
                }
                Ok(PyObject::none())
            }));
    }

    // __enter__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", move |_args| {
                Ok(build_bz2_file(st.clone()))
            }));
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let mut g = st.lock().unwrap();
                if !g.closed {
                    g.closed = true;
                    if g.mode.contains('w') {
                        let mut enc = bzip2::write::BzEncoder::new(
                            Vec::new(), bzip2::Compression::new(9),
                        );
                        let _ = enc.write_all(&g.buffer);
                        if let Ok(c) = enc.finish() {
                            let _ = std::fs::write(&g.filepath, &c);
                        }
                    }
                }
                Ok(PyObject::none())
            }));
    }

    let cls = PyObject::class(CompactString::from("BZ2File"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn bz2_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "bz2.open() missing required argument: 'filename'",
        ));
    }
    let filepath = args[0].py_to_string();
    let mode = if args.len() > 1 { args[1].py_to_string() } else { "rb".to_string() };

    let buffer = if mode.contains('r') {
        let raw = std::fs::read(&filepath).map_err(|e| {
            PyException::runtime_error(&format!("bz2.open: {e}"))
        })?;
        let mut dec = bzip2::read::BzDecoder::new(&raw[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).map_err(|e| {
            PyException::runtime_error(&format!("bz2.open: {e}"))
        })?;
        out
    } else {
        Vec::new()
    };

    Ok(build_bz2_file(Arc::new(Mutex::new(Bz2FileInner {
        mode, filepath, buffer, closed: false,
    }))))
}

fn bz2_compressor_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let level = if !args.is_empty() {
        args[0].as_int().unwrap_or(9) as u32
    } else {
        9
    };
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();

    {
        let b = buf.clone();
        attrs.insert(CompactString::from("compress"),
            PyObject::native_closure("compress", move |args| {
                if args.is_empty() { return Err(PyException::type_error("compress() requires data")); }
                let data = extract_bytes(&args[0])?;
                let mut enc = bzip2::write::BzEncoder::new(
                    Vec::new(), bzip2::Compression::new(level.min(9)),
                );
                enc.write_all(&data).map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                let out = enc.finish().map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                b.lock().unwrap().extend(&out);
                Ok(PyObject::bytes(out))
            }));
    }
    {
        let b = buf.clone();
        attrs.insert(CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let data = b.lock().unwrap().clone();
                Ok(PyObject::bytes(data))
            }));
    }

    let cls = PyObject::class(CompactString::from("BZ2Compressor"), vec![], IndexMap::new());
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn bz2_decompressor_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();

    {
        let b = buf.clone();
        attrs.insert(CompactString::from("decompress"),
            PyObject::native_closure("decompress", move |args| {
                if args.is_empty() { return Err(PyException::type_error("decompress() requires data")); }
                let data = extract_bytes(&args[0])?;
                let mut dec = bzip2::read::BzDecoder::new(&data[..]);
                let mut out = Vec::new();
                dec.read_to_end(&mut out).map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                b.lock().unwrap().extend(&out);
                Ok(PyObject::bytes(out))
            }));
    }
    attrs.insert(CompactString::from("eof"), PyObject::bool_val(false));
    attrs.insert(CompactString::from("needs_input"), PyObject::bool_val(true));
    attrs.insert(CompactString::from("unused_data"), PyObject::bytes(vec![]));

    let cls = PyObject::class(CompactString::from("BZ2Decompressor"), vec![], IndexMap::new());
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

// ══════════════════════════════════════════════════════════════════════
//  lzma module
// ══════════════════════════════════════════════════════════════════════

pub fn create_lzma_module() -> PyObjectRef {
    make_module("lzma", vec![
        ("compress", make_builtin(lzma_compress)),
        ("decompress", make_builtin(lzma_decompress)),
        ("open", make_builtin(lzma_open)),
        ("LZMACompressor", make_builtin(lzma_compressor_ctor)),
        ("LZMADecompressor", make_builtin(lzma_decompressor_ctor)),
        ("FORMAT_AUTO", PyObject::int(0)),
        ("FORMAT_XZ", PyObject::int(1)),
        ("FORMAT_ALONE", PyObject::int(2)),
        ("FORMAT_RAW", PyObject::int(3)),
        ("CHECK_NONE", PyObject::int(0)),
        ("CHECK_CRC32", PyObject::int(1)),
        ("CHECK_CRC64", PyObject::int(4)),
        ("CHECK_SHA256", PyObject::int(10)),
    ])
}

fn lzma_compress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "lzma.compress() missing required argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    let preset = if args.len() > 1 {
        args[1].as_int().unwrap_or(6) as u32
    } else {
        6
    };
    let mut encoder = xz2::write::XzEncoder::new(Vec::new(), preset.min(9));
    encoder.write_all(&data).map_err(|e| {
        PyException::runtime_error(&format!("lzma.compress: {e}"))
    })?;
    let compressed = encoder.finish().map_err(|e| {
        PyException::runtime_error(&format!("lzma.compress: {e}"))
    })?;
    Ok(PyObject::bytes(compressed))
}

fn lzma_decompress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "lzma.decompress() missing required argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    let mut decoder = xz2::read::XzDecoder::new(&data[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| {
        PyException::runtime_error(&format!("lzma.decompress: {e}"))
    })?;
    Ok(PyObject::bytes(out))
}

struct LzmaFileInner {
    mode: String,
    filepath: String,
    buffer: Vec<u8>,
    closed: bool,
}

fn build_lzma_file(inner: Arc<Mutex<LzmaFileInner>>) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__lzmafile__"), PyObject::bool_val(true));

    // name / mode / closed attributes
    {
        let g = inner.lock().unwrap();
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(g.filepath.as_str())));
        attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(g.mode.as_str())));
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(g.closed));
    }

    // read(size=-1)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("read"),
            PyObject::native_closure("read", move |args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                let size = if !args.is_empty() { args[0].as_int().unwrap_or(-1) } else { -1 };
                if size < 0 || size as usize >= g.buffer.len() {
                    Ok(PyObject::bytes(g.buffer.clone()))
                } else {
                    Ok(PyObject::bytes(g.buffer[..size as usize].to_vec()))
                }
            }));
    }

    // readline()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readline"),
            PyObject::native_closure("readline", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                match g.buffer.iter().position(|&b| b == b'\n') {
                    Some(i) => Ok(PyObject::bytes(g.buffer[..=i].to_vec())),
                    None => Ok(PyObject::bytes(g.buffer.clone())),
                }
            }));
    }

    // readlines()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readlines"),
            PyObject::native_closure("readlines", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                let mut lines = Vec::new();
                let mut start = 0;
                for (i, &b) in g.buffer.iter().enumerate() {
                    if b == b'\n' {
                        lines.push(PyObject::bytes(g.buffer[start..=i].to_vec()));
                        start = i + 1;
                    }
                }
                if start < g.buffer.len() {
                    lines.push(PyObject::bytes(g.buffer[start..].to_vec()));
                }
                Ok(PyObject::list(lines))
            }));
    }

    // write(data)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("write"),
            PyObject::native_closure("write", move |args| {
                if args.is_empty() { return Err(PyException::type_error("write() requires data")); }
                let data = extract_bytes(&args[0])?;
                let mut g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                let len = data.len();
                g.buffer.extend(data);
                Ok(PyObject::int(len as i64))
            }));
    }

    // flush()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                Ok(PyObject::none())
            }));
    }

    // tell()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("tell"),
            PyObject::native_closure("tell", move |_args| {
                let g = st.lock().unwrap();
                if g.closed { return Err(PyException::runtime_error("I/O operation on closed file")); }
                Ok(PyObject::int(g.buffer.len() as i64))
            }));
    }

    // seek()
    attrs.insert(CompactString::from("seek"),
        PyObject::native_closure("seek", move |_args| {
            Err(PyException::runtime_error("seek() not supported on lzma files"))
        }));

    // seekable() / readable() / writable()
    attrs.insert(CompactString::from("seekable"), make_builtin(|_| Ok(PyObject::bool_val(false))));
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("readable"),
            PyObject::native_closure("readable", move |_args| {
                let g = st.lock().unwrap();
                Ok(PyObject::bool_val(g.mode.contains('r')))
            }));
    }
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("writable"),
            PyObject::native_closure("writable", move |_args| {
                let g = st.lock().unwrap();
                Ok(PyObject::bool_val(g.mode.contains('w') || g.mode.contains('a')))
            }));
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                let mut g = st.lock().unwrap();
                if g.closed { return Ok(PyObject::none()); }
                g.closed = true;
                if g.mode.contains('w') {
                    let mut enc = xz2::write::XzEncoder::new(Vec::new(), 6);
                    enc.write_all(&g.buffer).map_err(|e| {
                        PyException::runtime_error(&format!("lzma close: {e}"))
                    })?;
                    let compressed = enc.finish().map_err(|e| {
                        PyException::runtime_error(&format!("lzma close: {e}"))
                    })?;
                    std::fs::write(&g.filepath, &compressed).map_err(|e| {
                        PyException::runtime_error(&format!("lzma close: {e}"))
                    })?;
                }
                Ok(PyObject::none())
            }));
    }

    // __enter__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", move |_args| {
                Ok(build_lzma_file(st.clone()))
            }));
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let mut g = st.lock().unwrap();
                if !g.closed {
                    g.closed = true;
                    if g.mode.contains('w') {
                        let mut enc = xz2::write::XzEncoder::new(Vec::new(), 6);
                        let _ = enc.write_all(&g.buffer);
                        if let Ok(c) = enc.finish() {
                            let _ = std::fs::write(&g.filepath, &c);
                        }
                    }
                }
                Ok(PyObject::none())
            }));
    }

    let cls = PyObject::class(CompactString::from("LZMAFile"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn lzma_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "lzma.open() missing required argument: 'filename'",
        ));
    }
    let filepath = args[0].py_to_string();
    let mode = if args.len() > 1 { args[1].py_to_string() } else { "rb".to_string() };

    let buffer = if mode.contains('r') {
        let raw = std::fs::read(&filepath).map_err(|e| {
            PyException::runtime_error(&format!("lzma.open: {e}"))
        })?;
        let mut dec = xz2::read::XzDecoder::new(&raw[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).map_err(|e| {
            PyException::runtime_error(&format!("lzma.open: {e}"))
        })?;
        out
    } else {
        Vec::new()
    };

    Ok(build_lzma_file(Arc::new(Mutex::new(LzmaFileInner {
        mode, filepath, buffer, closed: false,
    }))))
}

fn lzma_compressor_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let preset = if !args.is_empty() {
        args[0].as_int().unwrap_or(6) as u32
    } else {
        6
    };
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();

    {
        let b = buf.clone();
        attrs.insert(CompactString::from("compress"),
            PyObject::native_closure("compress", move |args| {
                if args.is_empty() { return Err(PyException::type_error("compress() requires data")); }
                let data = extract_bytes(&args[0])?;
                let mut enc = xz2::write::XzEncoder::new(Vec::new(), preset.min(9));
                enc.write_all(&data).map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                let out = enc.finish().map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                b.lock().unwrap().extend(&out);
                Ok(PyObject::bytes(out))
            }));
    }
    {
        let b = buf.clone();
        attrs.insert(CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let data = b.lock().unwrap().clone();
                Ok(PyObject::bytes(data))
            }));
    }

    let cls = PyObject::class(CompactString::from("LZMACompressor"), vec![], IndexMap::new());
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn lzma_decompressor_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();

    attrs.insert(CompactString::from("decompress"),
        PyObject::native_closure("decompress", move |args| {
            if args.is_empty() { return Err(PyException::type_error("decompress() requires data")); }
            let data = extract_bytes(&args[0])?;
            let mut dec = xz2::read::XzDecoder::new(&data[..]);
            let mut out = Vec::new();
            dec.read_to_end(&mut out).map_err(|e| PyException::runtime_error(&format!("{e}")))?;
            Ok(PyObject::bytes(out))
        }));
    attrs.insert(CompactString::from("eof"), PyObject::bool_val(false));
    attrs.insert(CompactString::from("needs_input"), PyObject::bool_val(true));
    attrs.insert(CompactString::from("check"), PyObject::int(0));
    attrs.insert(CompactString::from("unused_data"), PyObject::bytes(vec![]));

    let cls = PyObject::class(CompactString::from("LZMADecompressor"), vec![], IndexMap::new());
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

// ══════════════════════════════════════════════════════════════════════
//  tarfile module
// ══════════════════════════════════════════════════════════════════════

pub fn create_tarfile_module() -> PyObjectRef {
    make_module("tarfile", vec![
        ("open", make_builtin(tarfile_open)),
        ("TarFile", make_builtin(tarfile_open)),
        ("TarInfo", make_builtin(tarinfo_constructor)),
        ("is_tarfile", make_builtin(|args: &[PyObjectRef]| {
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
        })),
        ("ENCODING", PyObject::str_val(CompactString::from("utf-8"))),
        ("DEFAULT_FORMAT", PyObject::int(1)),  // GNU_FORMAT
        ("USTAR_FORMAT", PyObject::int(0)),
        ("GNU_FORMAT", PyObject::int(1)),
        ("PAX_FORMAT", PyObject::int(2)),
    ])
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
    attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));
    attrs.insert(CompactString::from("size"), PyObject::int(size as i64));
    attrs.insert(CompactString::from("mtime"), PyObject::int(0));
    attrs.insert(CompactString::from("mode"), PyObject::int(0o644));
    attrs.insert(CompactString::from("uid"), PyObject::int(0));
    attrs.insert(CompactString::from("gid"), PyObject::int(0));
    attrs.insert(CompactString::from("uname"), PyObject::str_val(CompactString::from("")));
    attrs.insert(CompactString::from("gname"), PyObject::str_val(CompactString::from("")));
    attrs.insert(CompactString::from("type"), if is_dir {
        PyObject::str_val(CompactString::from("5"))  // DIRTYPE
    } else {
        PyObject::str_val(CompactString::from("0"))  // REGTYPE
    });
    attrs.insert(CompactString::from("isdir"),
        PyObject::native_closure("isdir", {
            let d = is_dir;
            move |_args| Ok(PyObject::bool_val(d))
        }));
    attrs.insert(CompactString::from("isfile"),
        PyObject::native_closure("isfile", {
            let d = is_dir;
            move |_args| Ok(PyObject::bool_val(!d))
        }));

    let cls = PyObject::class(CompactString::from("TarInfo"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn build_tarfile_object(inner: Arc<Mutex<TarInner>>) -> PyObjectRef {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(CompactString::from("__tarfile__"), PyObject::bool_val(true));

    // getnames()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("getnames"),
            PyObject::native_closure("getnames", move |_args| {
                let g = st.lock().unwrap();
                let names: Vec<PyObjectRef> = g.entries.iter()
                    .map(|e| PyObject::str_val(CompactString::from(e.name.as_str())))
                    .collect();
                Ok(PyObject::list(names))
            }));
    }

    // getmembers()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("getmembers"),
            PyObject::native_closure("getmembers", move |_args| {
                let g = st.lock().unwrap();
                let members: Vec<PyObjectRef> = g.entries.iter()
                    .map(|e| build_tarinfo(&e.name, e.size, e.is_dir))
                    .collect();
                Ok(PyObject::list(members))
            }));
    }

    // getmember(name) → TarInfo
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("getmember"),
            PyObject::native_closure("getmember", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("getmember() requires name argument"));
                }
                let name = args[0].py_to_string();
                let g = st.lock().unwrap();
                for entry in &g.entries {
                    if entry.name == name {
                        return Ok(build_tarinfo(&entry.name, entry.size, entry.is_dir));
                    }
                }
                Err(PyException::key_error(&format!("KeyError: '{name}'")))
            }));
    }

    // extractall(path='.')
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("extractall"),
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
            }));
    }

    // extractfile(member)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("extractfile"),
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
                            file_attrs.insert(CompactString::from("read"),
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
                                }));
                        }
                        let cls = PyObject::class(CompactString::from("ExFileObject"), vec![], IndexMap::new());
                        return Ok(PyObject::instance_with_attrs(cls, file_attrs));
                    }
                }
                Err(PyException::key_error(&format!("KeyError: '{name}'")))
            }));
    }

    // add(name, arcname=None)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("add"),
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
                        r.get(&HashableKey::Str(CompactString::from("arcname")))
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
                    let data = std::fs::read(&filepath).map_err(|e| {
                        PyException::runtime_error(&format!("tarfile.add: {e}"))
                    })?;
                    let size = data.len() as u64;
                    g.entries.push(TarEntry {
                        name: arcname,
                        data,
                        size,
                        is_dir: false,
                    });
                }
                Ok(PyObject::none())
            }));
    }

    // addfile(tarinfo, fileobj=None)
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("addfile"),
            PyObject::native_closure("addfile", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("addfile() requires tarinfo"));
                }
                let name = args[0].get_attr("name")
                    .map(|n| n.py_to_string())
                    .unwrap_or_default();
                let data = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    // Try reading data from fileobj
                    if let PyObjectPayload::Bytes(b) = &args[1].payload {
                        b.clone()
                    } else {
                        extract_bytes_from_fileobj(&args[1]).unwrap_or_default()
                    }
                } else {
                    vec![]
                };
                let size = data.len() as u64;
                let mut g = st.lock().unwrap();
                g.entries.push(TarEntry {
                    name, data, size, is_dir: false,
                });
                Ok(PyObject::none())
            }));
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                let mut g = st.lock().unwrap();
                if g.closed { return Ok(PyObject::none()); }
                g.closed = true;
                if g.mode.contains('w') {
                    write_tar_to_disk(&g)?;
                }
                Ok(PyObject::none())
            }));
    }

    // __enter__ / __exit__
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", move |_args| {
                Ok(build_tarfile_object(st.clone()))
            }));
    }
    {
        let st = inner.clone();
        attrs.insert(CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let mut g = st.lock().unwrap();
                if !g.closed {
                    g.closed = true;
                    if g.mode.contains('w') {
                        let _ = write_tar_to_disk(&g);
                    }
                }
                Ok(PyObject::none())
            }));
    }

    // name attribute
    {
        let path = inner.lock().unwrap().filepath.clone();
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path.as_str())));
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
                tar_builder.append_data(&mut header, &entry.name, &[][..]).map_err(|e| {
                    PyException::runtime_error(&format!("tarfile: {e}"))
                })?;
            } else {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(entry.data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar_builder.append_data(&mut header, &entry.name, &entry.data[..]).map_err(|e| {
                    PyException::runtime_error(&format!("tarfile: {e}"))
                })?;
            }
        }
        let cursor = tar_builder.into_inner().map_err(|e| {
            PyException::runtime_error(&format!("tarfile: {e}"))
        })?;
        Ok(cursor.into_inner())
    };

    // Write to fileobj if present
    if let Some(ref fobj) = inner.fileobj {
        let data = build_tar_bytes(&inner.entries)?;
        // Write data back to the BytesIO by calling its write method
        if let Some(write_fn) = fobj.get_attr("write") {
            match &write_fn.payload {
                PyObjectPayload::NativeFunction { func, .. } => {
                    func(&[PyObject::bytes(data)])?;
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
                PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&[PyObject::int(0)]); }
                PyObjectPayload::NativeClosure(nc) => { let _ = (nc.func)(&[PyObject::int(0)]); }
                _ => {}
            }
        }
        return Ok(());
    }

    let filepath = &inner.filepath;
    let file = std::fs::File::create(filepath).map_err(|e| {
        PyException::runtime_error(&format!("tarfile.close: {e}"))
    })?;

    let writer: Box<dyn Write> = if filepath.ends_with(".gz") || filepath.ends_with(".tgz") {
        Box::new(flate2::write::GzEncoder::new(file, flate2::Compression::default()))
    } else if filepath.ends_with(".bz2") {
        Box::new(bzip2::write::BzEncoder::new(file, bzip2::Compression::default()))
    } else if filepath.ends_with(".xz") {
        Box::new(xz2::write::XzEncoder::new(file, 6))
    } else {
        Box::new(file)
    };

    let data = build_tar_bytes(&inner.entries)?;
    let mut writer = writer;
    writer.write_all(&data).map_err(|e| {
        PyException::runtime_error(&format!("tarfile: {e}"))
    })?;
    Ok(())
}

fn tarfile_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Parse kwargs from last arg if it's a Dict
    let kwargs = args.last().and_then(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload { Some(kw.clone()) } else { None }
    });
    let fileobj = kwargs.as_ref().and_then(|kw| {
        let r = kw.read();
        r.get(&HashableKey::Str(CompactString::from("fileobj"))).cloned()
    });
    let mode_kwarg = kwargs.as_ref().and_then(|kw| {
        let r = kw.read();
        r.get(&HashableKey::Str(CompactString::from("mode"))).map(|v| v.py_to_string())
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
            filepath: String::new(), mode, entries, closed: false,
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
        filepath, mode, entries, closed: false, fileobj: None,
    }))))
}

/// Extract raw bytes from a BytesIO or similar object
fn extract_bytes_from_fileobj(fobj: &PyObjectRef) -> PyResult<Vec<u8>> {
    // Try direct bytes payload
    if let PyObjectPayload::Bytes(b) = &fobj.payload {
        return Ok(b.clone());
    }
    // Try BytesIO: look for _buffer attribute
    if let Some(buf_attr) = fobj.get_attr("_buffer") {
        if let PyObjectPayload::Bytes(b) = &buf_attr.payload {
            return Ok(b.clone());
        }
    }
    // Try getvalue() method
    if let Some(getvalue) = fobj.get_attr("getvalue") {
        // Can't call Python method from native code, but NativeFunction is OK
        if let PyObjectPayload::NativeFunction { func, .. } = &getvalue.payload {
            let result = func(&[])?;
            if let PyObjectPayload::Bytes(b) = &result.payload {
                return Ok(b.clone());
            }
        }
        if let PyObjectPayload::NativeClosure(nc) = &getvalue.payload {
            let result = (nc.func)(&[])?;
            if let PyObjectPayload::Bytes(b) = &result.payload {
                return Ok(b.clone());
            }
        }
    }
    Err(PyException::type_error("fileobj must be a BytesIO or bytes-like object"))
}

fn read_tar_entries_from_bytes(data: &[u8]) -> PyResult<Vec<TarEntry>> {
    let reader = std::io::Cursor::new(data);
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    for entry_result in archive.entries().map_err(|e| {
        PyException::runtime_error(&format!("tarfile: {e}"))
    })? {
        let mut entry = entry_result.map_err(|e| {
            PyException::runtime_error(&format!("tarfile: {e}"))
        })?;
        let name = entry.path().map_err(|e| {
            PyException::runtime_error(&format!("tarfile: {e}"))
        })?.to_string_lossy().to_string();
        let is_dir = entry.header().entry_type().is_dir();
        let size = entry.size();
        let mut edata = Vec::new();
        if !is_dir {
            entry.read_to_end(&mut edata).map_err(|e| {
                PyException::runtime_error(&format!("tarfile: {e}"))
            })?;
        }
        entries.push(TarEntry { name, data: edata, size, is_dir });
    }
    Ok(entries)
}

fn read_tar_entries(filepath: &str) -> PyResult<Vec<TarEntry>> {
    let file = std::fs::File::open(filepath).map_err(|e| {
        PyException::runtime_error(&format!("tarfile.open: {e}"))
    })?;

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
    for entry_result in archive.entries().map_err(|e| {
        PyException::runtime_error(&format!("tarfile.open: {e}"))
    })? {
        let mut entry = entry_result.map_err(|e| {
            PyException::runtime_error(&format!("tarfile: {e}"))
        })?;
        let name = entry.path().map_err(|e| {
            PyException::runtime_error(&format!("tarfile: {e}"))
        })?.to_string_lossy().to_string();
        let is_dir = entry.header().entry_type().is_dir();
        let size = entry.size();
        let mut data = Vec::new();
        if !is_dir {
            entry.read_to_end(&mut data).map_err(|e| {
                PyException::runtime_error(&format!("tarfile: {e}"))
            })?;
        }
        entries.push(TarEntry { name, data, size, is_dir });
    }
    Ok(entries)
}

fn tarinfo_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Handle TarInfo(name=...) with kwargs or positional
    let (name, size) = if !args.is_empty() {
        if let PyObjectPayload::Dict(kw) = &args[0].payload {
            let r = kw.read();
            let n = r.get(&HashableKey::Str(CompactString::from("name")))
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            let s = r.get(&HashableKey::Str(CompactString::from("size")))
                .and_then(|v| v.as_int())
                .unwrap_or(0) as u64;
            (n, s)
        } else {
            let n = args[0].py_to_string();
            let s = if args.len() > 1 {
                if let PyObjectPayload::Dict(kw) = &args[1].payload {
                    let r = kw.read();
                    r.get(&HashableKey::Str(CompactString::from("size")))
                        .and_then(|v| v.as_int())
                        .unwrap_or(0) as u64
                } else {
                    args[1].as_int().unwrap_or(0) as u64
                }
            } else { 0 };
            (n, s)
        }
    } else {
        (String::new(), 0)
    };
    Ok(build_tarinfo(&name, size, false))
}
