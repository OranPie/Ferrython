//! File I/O builtins (open, read, write, close, context manager)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args,
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
    
    let content: Arc<RwLock<FileState>> = Arc::new(RwLock::new(FileState::new(&path, &mode)?));
    
    // Create a module-like object with file methods
    let mut attrs = IndexMap::new();
    let state = content.clone();
    attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path.clone())));
    attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode.clone())));
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(CompactString::from("_state"), PyObject::int(Arc::as_ptr(&state) as i64));
    
    // Store the file state globally so methods can access it
    FILE_STATES.lock().unwrap().insert(Arc::as_ptr(&state) as usize, state);
    
    let file_obj = PyObject::module_with_attrs(CompactString::from("_file"), attrs);
    // Add methods via NativeFunction
    match &file_obj.payload {
        PyObjectPayload::Module(md) => {
            // We can't mutate, so let's create a new module with all attrs
        }
        _ => {}
    }
    
    // Better approach: return a module with native function methods
    let ptr = Arc::as_ptr(&content) as i64;
    let mut all_attrs = IndexMap::new();
    all_attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path)));
    all_attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode)));
    all_attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    all_attrs.insert(CompactString::from("_ptr"), PyObject::int(ptr));
    all_attrs.insert(CompactString::from("read"), PyObject::native_function("read", file_read));
    all_attrs.insert(CompactString::from("readline"), PyObject::native_function("readline", file_readline));
    all_attrs.insert(CompactString::from("readlines"), PyObject::native_function("readlines", file_readlines));
    all_attrs.insert(CompactString::from("write"), PyObject::native_function("write", file_write));
    all_attrs.insert(CompactString::from("writelines"), PyObject::native_function("writelines", file_writelines));
    all_attrs.insert(CompactString::from("close"), PyObject::native_function("close", file_close));
    all_attrs.insert(CompactString::from("__enter__"), PyObject::native_function("__enter__", file_enter));
    all_attrs.insert(CompactString::from("__exit__"), PyObject::native_function("__exit__", file_exit));
    
    // Store file state associated with the ptr value
    CURRENT_FILE_STATE.lock().unwrap().replace(content);
    
    all_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    
    Ok(PyObject::module_with_attrs(CompactString::from("_file"), all_attrs))
}

static FILE_STATES: std::sync::LazyLock<Mutex<IndexMap<usize, Arc<RwLock<FileState>>>>> = 
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

static CURRENT_FILE_STATE: std::sync::LazyLock<Mutex<Option<Arc<RwLock<FileState>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

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

fn get_current_file() -> PyResult<Arc<RwLock<FileState>>> {
    CURRENT_FILE_STATE.lock().unwrap().clone().ok_or_else(|| {
        PyException::value_error("I/O operation on closed file")
    })
}

pub(super) fn file_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let max_bytes = if !args.is_empty() { 
        let n = args[0].to_int()?;
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
    let state = get_current_file()?;
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

pub(super) fn file_readlines(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
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
    check_args("write", args, 1)?;
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let text = args[0].py_to_string();
    let len = text.len();
    s.write_buf.push_str(&text);
    Ok(PyObject::int(len as i64))
}

pub(super) fn file_writelines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("writelines", args, 1)?;
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let items = args[0].to_list()?;
    for item in items {
        s.write_buf.push_str(&item.py_to_string());
    }
    Ok(PyObject::none())
}

pub(super) fn file_close(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
    let mut s = state.write();
    if !s.closed {
        // Flush write buffer
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

pub(super) fn file_exit(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    file_close(&[])?;
    Ok(PyObject::bool_val(false))
}
