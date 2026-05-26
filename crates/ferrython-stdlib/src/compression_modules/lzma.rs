//! lzma module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use super::extract_bytes;

// ══════════════════════════════════════════════════════════════════════
//  lzma module
// ══════════════════════════════════════════════════════════════════════

pub fn create_lzma_module() -> PyObjectRef {
    make_module(
        "lzma",
        vec![
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
        ],
    )
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
    encoder
        .write_all(&data)
        .map_err(|e| PyException::runtime_error(&format!("lzma.compress: {e}")))?;
    let compressed = encoder
        .finish()
        .map_err(|e| PyException::runtime_error(&format!("lzma.compress: {e}")))?;
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
    decoder
        .read_to_end(&mut out)
        .map_err(|e| PyException::runtime_error(&format!("lzma.decompress: {e}")))?;
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
    attrs.insert(
        CompactString::from("__lzmafile__"),
        PyObject::bool_val(true),
    );

    // name / mode / closed attributes
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
                let g = st.lock().unwrap();
                if g.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let size = if !args.is_empty() {
                    args[0].as_int().unwrap_or(-1)
                } else {
                    -1
                };
                if size < 0 || size as usize >= g.buffer.len() {
                    Ok(PyObject::bytes(g.buffer.clone()))
                } else {
                    Ok(PyObject::bytes(g.buffer[..size as usize].to_vec()))
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
                let g = st.lock().unwrap();
                if g.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                match g.buffer.iter().position(|&b| b == b'\n') {
                    Some(i) => Ok(PyObject::bytes(g.buffer[..=i].to_vec())),
                    None => Ok(PyObject::bytes(g.buffer.clone())),
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
                let g = st.lock().unwrap();
                if g.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
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
                    return Err(PyException::type_error("write() requires data"));
                }
                let data = extract_bytes(&args[0])?;
                let mut g = st.lock().unwrap();
                if g.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                let len = data.len();
                g.buffer.extend(data);
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
                let g = st.lock().unwrap();
                if g.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                Ok(PyObject::none())
            }),
        );
    }

    // tell()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("tell", move |_args| {
                let g = st.lock().unwrap();
                if g.closed {
                    return Err(PyException::runtime_error("I/O operation on closed file"));
                }
                Ok(PyObject::int(g.buffer.len() as i64))
            }),
        );
    }

    // seek()
    attrs.insert(
        CompactString::from("seek"),
        PyObject::native_closure("seek", move |_args| {
            Err(PyException::runtime_error(
                "seek() not supported on lzma files",
            ))
        }),
    );

    // seekable() / readable() / writable()
    attrs.insert(
        CompactString::from("seekable"),
        make_builtin(|_| Ok(PyObject::bool_val(false))),
    );
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("readable"),
            PyObject::native_closure("readable", move |_args| {
                let g = st.lock().unwrap();
                Ok(PyObject::bool_val(g.mode.contains('r')))
            }),
        );
    }
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("writable"),
            PyObject::native_closure("writable", move |_args| {
                let g = st.lock().unwrap();
                Ok(PyObject::bool_val(
                    g.mode.contains('w') || g.mode.contains('a'),
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
                let mut g = st.lock().unwrap();
                if g.closed {
                    return Ok(PyObject::none());
                }
                g.closed = true;
                if g.mode.contains('w') {
                    let mut enc = xz2::write::XzEncoder::new(Vec::new(), 6);
                    enc.write_all(&g.buffer)
                        .map_err(|e| PyException::runtime_error(&format!("lzma close: {e}")))?;
                    let compressed = enc
                        .finish()
                        .map_err(|e| PyException::runtime_error(&format!("lzma close: {e}")))?;
                    std::fs::write(&g.filepath, &compressed)
                        .map_err(|e| PyException::runtime_error(&format!("lzma close: {e}")))?;
                }
                Ok(PyObject::none())
            }),
        );
    }

    // __enter__
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", move |_args| Ok(build_lzma_file(st.clone()))),
        );
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("__exit__"),
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
            }),
        );
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
    let mode = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "rb".to_string()
    };

    let buffer = if mode.contains('r') {
        let raw = std::fs::read(&filepath)
            .map_err(|e| PyException::runtime_error(&format!("lzma.open: {e}")))?;
        let mut dec = xz2::read::XzDecoder::new(&raw[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out)
            .map_err(|e| PyException::runtime_error(&format!("lzma.open: {e}")))?;
        out
    } else {
        Vec::new()
    };

    Ok(build_lzma_file(Arc::new(Mutex::new(LzmaFileInner {
        mode,
        filepath,
        buffer,
        closed: false,
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
        attrs.insert(
            CompactString::from("compress"),
            PyObject::native_closure("compress", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("compress() requires data"));
                }
                let data = extract_bytes(&args[0])?;
                let mut enc = xz2::write::XzEncoder::new(Vec::new(), preset.min(9));
                enc.write_all(&data)
                    .map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                let out = enc
                    .finish()
                    .map_err(|e| PyException::runtime_error(&format!("{e}")))?;
                b.lock().unwrap().extend(&out);
                Ok(PyObject::bytes(out))
            }),
        );
    }
    {
        let b = buf.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("flush", move |_args| {
                let data = b.lock().unwrap().clone();
                Ok(PyObject::bytes(data))
            }),
        );
    }

    let cls = PyObject::class(
        CompactString::from("LZMACompressor"),
        vec![],
        IndexMap::new(),
    );
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn lzma_decompressor_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();

    attrs.insert(
        CompactString::from("decompress"),
        PyObject::native_closure("decompress", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("decompress() requires data"));
            }
            let data = extract_bytes(&args[0])?;
            let mut dec = xz2::read::XzDecoder::new(&data[..]);
            let mut out = Vec::new();
            dec.read_to_end(&mut out)
                .map_err(|e| PyException::runtime_error(&format!("{e}")))?;
            Ok(PyObject::bytes(out))
        }),
    );
    attrs.insert(CompactString::from("eof"), PyObject::bool_val(false));
    attrs.insert(CompactString::from("needs_input"), PyObject::bool_val(true));
    attrs.insert(CompactString::from("check"), PyObject::int(0));
    attrs.insert(CompactString::from("unused_data"), PyObject::bytes(vec![]));

    let cls = PyObject::class(
        CompactString::from("LZMADecompressor"),
        vec![],
        IndexMap::new(),
    );
    Ok(PyObject::instance_with_attrs(cls, attrs))
}
