//! gzip module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use super::extract_bytes;

// ══════════════════════════════════════════════════════════════════════
//  gzip module
// ══════════════════════════════════════════════════════════════════════

pub fn create_gzip_module() -> PyObjectRef {
    make_module(
        "gzip",
        vec![
            ("compress", make_builtin(gzip_compress)),
            ("decompress", make_builtin(gzip_decompress)),
            ("open", make_builtin(gzip_open)),
            ("GzipFile", make_builtin(gzip_file_constructor)),
            (
                "BadGzipFile",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
        ],
    )
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
    encoder
        .write_all(&data)
        .map_err(|e| PyException::runtime_error(&format!("gzip.compress: {}", e)))?;
    let compressed = encoder
        .finish()
        .map_err(|e| PyException::runtime_error(&format!("gzip.compress: {}", e)))?;

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
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| PyException::runtime_error(&format!("gzip.decompress: {}", e)))?;

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
    attrs.insert(
        CompactString::from("__gzipfile__"),
        PyObject::bool_val(true),
    );

    // name attribute
    {
        let g = inner.lock().unwrap();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(g.filepath.as_str())),
        );
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from(g.mode.as_str())),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(g.closed));
    }

    // read(size=-1)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("read", move |args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let size = if !args.is_empty() {
                    args[0].as_int().unwrap_or(-1)
                } else {
                    -1
                };
                if size < 0 || size as usize >= guard.buffer.len() {
                    Ok(PyObject::bytes(guard.buffer.clone()))
                } else {
                    Ok(PyObject::bytes(guard.buffer[..size as usize].to_vec()))
                }
            }),
        );
    }

    // readline()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("readline"),
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
            }),
        );
    }

    // readlines()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("readlines"),
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
            }),
        );
    }

    // write(data)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("write"),
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
            }),
        );
    }

    // flush()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                Ok(PyObject::none())
            }),
        );
    }

    // seek(offset, whence=0)
    {
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("seek", move |_args| {
                Err(PyException::runtime_error(
                    "seek() not supported on gzip files",
                ))
            }),
        );
    }

    // tell()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("tell", move |_args| {
                let guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                Ok(PyObject::int(guard.buffer.len() as i64))
            }),
        );
    }

    // seekable()
    attrs.insert(
        CompactString::from("seekable"),
        make_builtin(|_| Ok(PyObject::bool_val(false))),
    );

    // readable()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("readable"),
            PyObject::native_closure("readable", move |_args| {
                let guard = st.lock().unwrap();
                Ok(PyObject::bool_val(guard.mode.contains('r')))
            }),
        );
    }

    // writable()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("writable"),
            PyObject::native_closure("writable", move |_args| {
                let guard = st.lock().unwrap();
                Ok(PyObject::bool_val(
                    guard.mode.contains('w') || guard.mode.contains('a'),
                ))
            }),
        );
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("close", move |_args| {
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Ok(PyObject::none());
                }
                guard.closed = true;
                if guard.mode.contains('w') {
                    let compression = flate2::Compression::new(9);
                    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), compression);
                    encoder
                        .write_all(&guard.buffer)
                        .map_err(|e| PyException::runtime_error(&format!("gzip close: {}", e)))?;
                    let compressed = encoder
                        .finish()
                        .map_err(|e| PyException::runtime_error(&format!("gzip close: {}", e)))?;
                    std::fs::write(&guard.filepath, &compressed)
                        .map_err(|e| PyException::runtime_error(&format!("gzip close: {}", e)))?;
                }
                Ok(PyObject::none())
            }),
        );
    }

    // __enter__
    {
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", {
                let st = inner.clone();
                move |_args| Ok(build_gzip_file_object(st.clone()))
            }),
        );
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("__exit__"),
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
            }),
        );
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
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("filename"))) {
                if !matches!(&v.payload, PyObjectPayload::None) {
                    filepath = v.py_to_string();
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("mode"))) {
                mode = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("fileobj"))) {
                if !matches!(&v.payload, PyObjectPayload::None) {
                    fileobj = Some(v.clone());
                }
            }
        } else {
            match i {
                0 => {
                    if !matches!(&arg.payload, PyObjectPayload::None) {
                        filepath = arg.py_to_string();
                    }
                }
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
            decoder
                .read_to_end(&mut out)
                .map_err(|e| PyException::runtime_error(&format!("GzipFile: {}", e)))?;
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
        return Err(PyException::type_error(
            "GzipFile requires filename or fileobj",
        ));
    }
    gzip_open_with(&filepath, &mode)
}

fn gzip_open_with(filepath: &str, mode: &str) -> PyResult<PyObjectRef> {
    let buffer = if mode.contains('r') {
        let raw = std::fs::read(filepath)
            .map_err(|e| PyException::runtime_error(&format!("GzipFile: {}", e)))?;
        let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| PyException::runtime_error(&format!("GzipFile: {}", e)))?;
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
