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
    all_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    
    Ok(PyObject::module_with_attrs(CompactString::from("_file"), all_attrs))
}

static FILE_STATES: std::sync::LazyLock<Mutex<IndexMap<usize, Arc<RwLock<FileState>>>>> = 
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

struct FileState {
    content: String,
    position: usize,
    mode: String,
    path: String,
    closed: bool,
    write_buf: String,
}

impl FileState {
    fn new(path: &str, mode: &str) -> PyResult<Self> {
        let content = if mode.contains('r') || mode.contains('+') {
            if mode.contains('r') && !std::path::Path::new(path).exists() {
                return Err(PyException::os_error(format!(
                    "[Errno 2] No such file or directory: '{}'", path
                )));
            }
            std::fs::read_to_string(path).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(Self {
            content,
            position: 0,
            mode: mode.to_string(),
            path: path.to_string(),
            closed: false,
            write_buf: String::new(),
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
    // args[0]=self, args[1]=optional max_bytes
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
    // args[0]=self, args[1]=text
    if args.len() < 2 {
        return Err(PyException::type_error("write() missing required argument: 'data'"));
    }
    let state = get_file_state(args)?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let text = args[1].py_to_string();
    let len = text.len();
    s.write_buf.push_str(&text);
    Ok(PyObject::int(len as i64))
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
        if !s.write_buf.is_empty() {
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
