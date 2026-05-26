//! zipfile module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use super::extract_bytes;

// ══════════════════════════════════════════════════════════════════════
//  zipfile module
// ══════════════════════════════════════════════════════════════════════

pub fn create_zipfile_module() -> PyObjectRef {
    make_module(
        "zipfile",
        vec![
            ("ZipFile", make_builtin(zipfile_constructor)),
            ("ZipInfo", make_builtin(zipinfo_constructor)),
            ("ZIP_STORED", PyObject::int(0)),
            ("ZIP_DEFLATED", PyObject::int(8)),
        ],
    )
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
    attrs.insert(
        CompactString::from("filename"),
        PyObject::str_val(CompactString::from(filename)),
    );
    attrs.insert(CompactString::from("file_size"), PyObject::int(size as i64));
    attrs.insert(
        CompactString::from("compress_size"),
        PyObject::int(size as i64),
    );
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
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("write", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "write() requires a filename argument",
                    ));
                }
                let filename = args[0].py_to_string();
                let arcname = if args.len() > 1 {
                    args[1].py_to_string()
                } else {
                    filename.clone()
                };
                let data = std::fs::read(&filename)
                    .map_err(|e| PyException::runtime_error(&format!("zipfile.write: {}", e)))?;
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error(
                        "zipfile: I/O operation on closed file",
                    ));
                }
                guard.entries.insert(arcname, data);
                Ok(PyObject::none())
            }),
        );
    }

    // writestr(arcname, data)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("writestr"),
            PyObject::native_closure("writestr", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "writestr() requires arcname and data",
                    ));
                }
                let arcname = args[0].py_to_string();
                let data =
                    extract_bytes(&args[1]).unwrap_or_else(|_| args[1].py_to_string().into_bytes());
                let mut guard = st.lock().unwrap();
                if guard.closed {
                    return Err(PyException::runtime_error(
                        "zipfile: I/O operation on closed file",
                    ));
                }
                guard.entries.insert(arcname, data);
                Ok(PyObject::none())
            }),
        );
    }

    // namelist()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("namelist"),
            PyObject::native_closure("namelist", move |_args| {
                let guard = st.lock().unwrap();
                let names: Vec<PyObjectRef> = guard
                    .entries
                    .keys()
                    .map(|k| PyObject::str_val(CompactString::from(k.as_str())))
                    .collect();
                Ok(PyObject::list(names))
            }),
        );
    }

    // read(name)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("read", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("read() requires a name argument"));
                }
                let name = args[0].py_to_string();
                let guard = st.lock().unwrap();
                match guard.entries.get(&name) {
                    Some(data) => Ok(PyObject::bytes(data.clone())),
                    None => Err(PyException::key_error(&format!(
                        "There is no item named '{}' in the archive",
                        name
                    ))),
                }
            }),
        );
    }

    // infolist()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("infolist"),
            PyObject::native_closure("infolist", move |_args| {
                let guard = st.lock().unwrap();
                let infos: Vec<PyObjectRef> = guard
                    .entries
                    .iter()
                    .map(|(name, data)| build_zipinfo(name, data.len()))
                    .collect();
                Ok(PyObject::list(infos))
            }),
        );
    }

    // getinfo(name)
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("getinfo"),
            PyObject::native_closure("getinfo", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "getinfo() requires a name argument",
                    ));
                }
                let name = args[0].py_to_string();
                let guard = st.lock().unwrap();
                match guard.entries.get(&name) {
                    Some(data) => Ok(build_zipinfo(&name, data.len())),
                    None => Err(PyException::key_error(&format!(
                        "There is no item named '{}' in the archive",
                        name
                    ))),
                }
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
                let guard = st.lock().unwrap();
                for (name, data) in guard.entries.iter() {
                    let path = std::path::Path::new(&dest).join(name);
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&path, data)
                        .map_err(|e| PyException::runtime_error(&format!("extractall: {}", e)))?;
                }
                Ok(PyObject::none())
            }),
        );
    }

    // close()
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("close", move |_args| zip_close_inner(&st)),
        );
    }

    // __enter__
    {
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("__enter__", {
                let st = inner.clone();
                move |_args| Ok(build_zipfile_object(st.clone()))
            }),
        );
    }

    // __exit__
    {
        let st = inner.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_args| {
                let _ = zip_close_inner(&st);
                Ok(PyObject::none())
            }),
        );
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
        let file = std::fs::File::create(&guard.filepath)
            .map_err(|e| PyException::runtime_error(&format!("zipfile.close: {}", e)))?;
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in guard.entries.iter() {
            writer
                .start_file(name, options)
                .map_err(|e| PyException::runtime_error(&format!("zipfile.close: {}", e)))?;
            writer
                .write_all(data)
                .map_err(|e| PyException::runtime_error(&format!("zipfile.close: {}", e)))?;
        }
        writer
            .finish()
            .map_err(|e| PyException::runtime_error(&format!("zipfile.close: {}", e)))?;
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
        let file = std::fs::File::open(&filepath)
            .map_err(|e| PyException::runtime_error(&format!("zipfile: {}", e)))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| PyException::runtime_error(&format!("zipfile: {}", e)))?;
        let mut map = IndexMap::new();
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| PyException::runtime_error(&format!("zipfile: {}", e)))?;
            let name = entry.name().to_string();
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|e| PyException::runtime_error(&format!("zipfile: {}", e)))?;
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
