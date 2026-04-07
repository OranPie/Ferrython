//! File I/O builtins (open, read, write, close, context manager)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::{Arc, Mutex};

// ── File I/O ──

pub(super) fn builtin_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("open() missing required argument: 'file'"));
    }
    let path = args[0].py_to_string();
    let mode = if args.len() > 1 { args[1].py_to_string() } else { "r".to_string() };
    
    let state: Arc<RwLock<FileState>> = Arc::new(RwLock::new(FileState::new(&path, &mode)?));
    let ptr = Arc::as_ptr(&state) as usize;
    
    // Register state globally so bound methods can find it via _ptr
    FILE_STATES.lock().unwrap().insert(ptr, state);
    
    let mut all_attrs = IndexMap::new();
    all_attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path)));
    all_attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode)));
    all_attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    all_attrs.insert(CompactString::from("_ptr"), PyObject::int(ptr as i64));
    all_attrs.insert(CompactString::from("read"), PyObject::native_function("read", file_read));
    all_attrs.insert(CompactString::from("readline"), PyObject::native_function("readline", file_readline));
    all_attrs.insert(CompactString::from("readlines"), PyObject::native_function("readlines", file_readlines));
    all_attrs.insert(CompactString::from("write"), PyObject::native_function("write", file_write));
    all_attrs.insert(CompactString::from("writelines"), PyObject::native_function("writelines", file_writelines));
    all_attrs.insert(CompactString::from("close"), PyObject::native_function("close", file_close));
    all_attrs.insert(CompactString::from("__enter__"), PyObject::native_function("__enter__", file_enter));
    all_attrs.insert(CompactString::from("__exit__"), PyObject::native_function("__exit__", file_exit));
    all_attrs.insert(CompactString::from("seek"), PyObject::native_function("seek", file_seek));
    all_attrs.insert(CompactString::from("tell"), PyObject::native_function("tell", file_tell));
    all_attrs.insert(CompactString::from("flush"), PyObject::native_function("flush", file_flush));
    all_attrs.insert(CompactString::from("truncate"), PyObject::native_function("truncate", file_truncate));
    all_attrs.insert(CompactString::from("readable"), PyObject::native_function("readable", file_readable));
    all_attrs.insert(CompactString::from("writable"), PyObject::native_function("writable", file_writable));
    all_attrs.insert(CompactString::from("seekable"), PyObject::native_function("seekable", file_seekable));
    all_attrs.insert(CompactString::from("__iter__"), PyObject::native_function("__iter__", file_iter));
    all_attrs.insert(CompactString::from("__next__"), PyObject::native_function("__next__", file_next));
    all_attrs.insert(CompactString::from("isatty"), PyObject::native_function("isatty", file_isatty));
    all_attrs.insert(CompactString::from("fileno"), PyObject::native_function("fileno", file_fileno));
    all_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    
    Ok(PyObject::module_with_attrs(CompactString::from("_file"), all_attrs))
}

static FILE_STATES: std::sync::LazyLock<Mutex<IndexMap<usize, Arc<RwLock<FileState>>>>> = 
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

struct FileState {
    content: String,
    binary_content: Vec<u8>,
    position: usize,
    mode: String,
    path: String,
    closed: bool,
    write_buf: String,
    binary_write_buf: Vec<u8>,
}

impl FileState {
    fn new(path: &str, mode: &str) -> PyResult<Self> {
        let is_binary = mode.contains('b');
        let (content, binary_content) = if mode.contains('r') || mode.contains('+') {
            if mode.contains('r') {
                // Use from_io_error for proper errno/strerror/filename attributes
                if let Err(e) = std::fs::metadata(path) {
                    return Err(PyException::from_io_error(&e, Some(path)));
                }
            }
            if is_binary {
                let bytes = std::fs::read(path).unwrap_or_default();
                (String::new(), bytes)
            } else {
                let text = std::fs::read_to_string(path).unwrap_or_default();
                (text, Vec::new())
            }
        } else {
            // "w" or "a" mode: create/truncate file on disk immediately
            if mode.contains('w') {
                let _ = std::fs::write(path, "");
            } else if mode.contains('a') {
                // Append mode: create if not exists
                let _ = std::fs::OpenOptions::new().create(true).append(true).open(path);
            }
            (String::new(), Vec::new())
        };
        Ok(Self {
            content,
            binary_content,
            position: 0,
            mode: mode.to_string(),
            path: path.to_string(),
            closed: false,
            write_buf: String::new(),
            binary_write_buf: Vec::new(),
        })
    }
}

/// Extract FileState from the bound self argument (args[0]._ptr → FILE_STATES lookup).
fn get_file_state(args: &[PyObjectRef]) -> PyResult<Arc<RwLock<FileState>>> {
    let self_obj = args.first().ok_or_else(|| {
        PyException::type_error("file method called without self")
    })?;
    let ptr_val = self_obj.get_attr("_ptr").ok_or_else(|| {
        PyException::type_error("not a file object")
    })?;
    let ptr = ptr_val.to_int()? as usize;
    FILE_STATES.lock().unwrap().get(&ptr).cloned().ok_or_else(|| {
        PyException::value_error("I/O operation on closed file")
    })
}

pub(super) fn file_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let is_binary = s.mode.contains('b');
    if is_binary {
        let max_bytes = if args.len() > 1 {
            let n = args[1].to_int()?;
            if n < 0 { s.binary_content.len() } else { n as usize }
        } else {
            s.binary_content.len()
        };
        let end = (s.position + max_bytes).min(s.binary_content.len());
        let result = s.binary_content[s.position..end].to_vec();
        s.position = end;
        Ok(PyObject::bytes(result))
    } else {
        let max_bytes = if args.len() > 1 { 
            let n = args[1].to_int()?;
            if n < 0 { s.content.len() } else { n as usize }
        } else { 
            s.content.len() 
        };
        let end = (s.position + max_bytes).min(s.content.len());
        let result = s.content[s.position..end].to_string();
        s.position = end;
        Ok(PyObject::str_val(CompactString::from(result)))
    }
}

pub(super) fn file_readline(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    if s.position >= s.content.len() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    let rest = &s.content[s.position..];
    let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let line = rest[..line_end].to_string();
    s.position += line_end;
    Ok(PyObject::str_val(CompactString::from(line)))
}

pub(super) fn file_readlines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let rest = &s.content[s.position..];
    let lines: Vec<PyObjectRef> = rest.lines()
        .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
        .collect();
    s.position = s.content.len();
    Ok(PyObject::list(lines))
}

pub(super) fn file_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0]=self, args[1]=text or bytes
    if args.len() < 2 {
        return Err(PyException::type_error("write() missing required argument: 'data'"));
    }
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    if s.mode.contains('b') {
        // Binary write: store raw bytes
        let data = match &args[1].payload {
            PyObjectPayload::Bytes(b) => b.clone(),
            PyObjectPayload::ByteArray(b) => b.clone(),
            _ => args[1].py_to_string().into_bytes(),
        };
        let len = data.len();
        s.binary_write_buf.extend_from_slice(&data);
        Ok(PyObject::int(len as i64))
    } else {
        let text = args[1].py_to_string();
        let len = text.len();
        s.write_buf.push_str(&text);
        Ok(PyObject::int(len as i64))
    }
}

pub(super) fn file_writelines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0]=self, args[1]=lines
    if args.len() < 2 {
        return Err(PyException::type_error("writelines() missing required argument: 'lines'"));
    }
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let items = args[1].to_list()?;
    for item in items {
        s.write_buf.push_str(&item.py_to_string());
    }
    Ok(PyObject::none())
}

pub(super) fn file_close(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if !s.closed {
        if s.mode.contains('b') {
            // Binary mode: flush binary_write_buf
            if !s.binary_write_buf.is_empty() {
                if s.mode.contains('a') {
                    let mut existing = std::fs::read(&s.path).unwrap_or_default();
                    existing.extend_from_slice(&s.binary_write_buf);
                    std::fs::write(&s.path, &existing)
                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                } else {
                    std::fs::write(&s.path, &s.binary_write_buf)
                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                }
                s.binary_write_buf.clear();
            }
        } else if !s.write_buf.is_empty() {
            if s.mode.contains('a') {
                let mut content = std::fs::read_to_string(&s.path).unwrap_or_default();
                content.push_str(&s.write_buf);
                std::fs::write(&s.path, &content)
                    .map_err(|e| PyException::os_error(format!("{}", e)))?;
            } else {
                std::fs::write(&s.path, &s.write_buf)
                    .map_err(|e| PyException::os_error(format!("{}", e)))?;
            }
            s.write_buf.clear();
        }
        s.closed = true;
    }
    // Update the Python-visible `closed` attribute on the file object
    if let Some(self_obj) = args.first() {
        if let PyObjectPayload::Module(ref md) = self_obj.payload {
            md.attrs.write().insert(CompactString::from("closed"), PyObject::bool_val(true));
        }
    }
    Ok(PyObject::none())
}

pub(super) fn file_enter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // __enter__ returns self (the file object)
    if let Some(self_obj) = args.first() {
        Ok(self_obj.clone())
    } else {
        Ok(PyObject::none())
    }
}

pub(super) fn file_exit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // __exit__(self, exc_type, exc_val, exc_tb) — close file, return False
    file_close(args)?;
    Ok(PyObject::bool_val(false))
}

pub(super) fn file_seek(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("seek() missing required argument: 'offset'"));
    }
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let offset = args[1].to_int()?;
    let whence = if args.len() > 2 { args[2].to_int()? } else { 0 };
    let is_binary = s.mode.contains('b');
    let data_len = if is_binary { s.binary_content.len() } else { s.content.len() };
    let new_pos = match whence {
        0 => offset.max(0) as usize,  // SEEK_SET
        1 => (s.position as i64 + offset).max(0) as usize,  // SEEK_CUR
        2 => (data_len as i64 + offset).max(0) as usize,  // SEEK_END
        _ => return Err(PyException::value_error("invalid whence value")),
    };
    s.position = new_pos;
    Ok(PyObject::int(s.position as i64))
}

pub(super) fn file_tell(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let s = state.read();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    Ok(PyObject::int(s.position as i64))
}

pub(super) fn file_flush(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    if s.mode.contains('b') {
        if !s.binary_write_buf.is_empty() {
            if s.mode.contains('a') {
                let mut existing = std::fs::read(&s.path).unwrap_or_default();
                existing.extend_from_slice(&s.binary_write_buf);
                std::fs::write(&s.path, &existing)
                    .map_err(|e| PyException::os_error(format!("{}", e)))?;
            } else {
                std::fs::write(&s.path, &s.binary_write_buf)
                    .map_err(|e| PyException::os_error(format!("{}", e)))?;
            }
            s.binary_write_buf.clear();
        }
    } else if !s.write_buf.is_empty() {
        if s.mode.contains('a') {
            let mut content = std::fs::read_to_string(&s.path).unwrap_or_default();
            content.push_str(&s.write_buf);
            std::fs::write(&s.path, &content)
                .map_err(|e| PyException::os_error(format!("{}", e)))?;
        } else {
            std::fs::write(&s.path, &s.write_buf)
                .map_err(|e| PyException::os_error(format!("{}", e)))?;
        }
        s.write_buf.clear();
    }
    Ok(PyObject::none())
}

pub(super) fn file_truncate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let size = if args.len() > 1 { args[1].to_int()? as usize } else { s.position };
    s.content.truncate(size);
    if s.position > size { s.position = size; }
    Ok(PyObject::int(size as i64))
}

pub(super) fn file_readable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let s = state.read();
    Ok(PyObject::bool_val(s.mode.contains('r') || s.mode.contains('+')))
}

pub(super) fn file_writable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let s = state.read();
    Ok(PyObject::bool_val(s.mode.contains('w') || s.mode.contains('a') || s.mode.contains('+')))
}

pub(super) fn file_seekable(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(true))
}

/// __iter__: returns self (file objects are their own iterators)
pub(super) fn file_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("__iter__ called without self"));
    }
    Ok(args[0].clone())
}

/// __next__: reads next line, raises StopIteration at EOF
pub(super) fn file_next(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed {
        return Err(PyException::value_error("I/O operation on closed file"));
    }
    if s.position >= s.content.len() {
        return Err(PyException::stop_iteration());
    }
    let rest = &s.content[s.position..];
    let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let line = rest[..line_end].to_string();
    s.position += line_end;
    if line.is_empty() {
        return Err(PyException::stop_iteration());
    }
    Ok(PyObject::str_val(CompactString::from(line)))
}

/// isatty: always returns False for file objects
pub(super) fn file_isatty(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(false))
}

/// fileno: open real OS file descriptor for the file path
pub(super) fn file_fileno(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_file_state(args)?;
    let s = state.read();
    if s.closed {
        return Err(PyException::value_error("I/O operation on closed file"));
    }
    use std::os::unix::io::IntoRawFd;
    let f = if s.mode.contains('w') || s.mode.contains('a') || s.mode.contains('+') {
        std::fs::OpenOptions::new().read(true).write(true).open(&s.path)
    } else {
        std::fs::File::open(&s.path)
    };
    match f {
        Ok(file) => Ok(PyObject::int(file.into_raw_fd() as i64)),
        Err(e) => Err(PyException::os_error(format!("{}: '{}'", e, s.path))),
    }
}
