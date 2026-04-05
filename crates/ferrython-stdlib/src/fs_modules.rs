//! Filesystem and process stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::{Arc, Mutex};

/// Extract the `_path` string from a pathlib instance (args[0] = self).
fn get_path_str(inst: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        if let Some(p) = data.attrs.read().get("_path") {
            return p.py_to_string();
        }
    }
    ".".to_string()
}

pub fn create_pathlib_module() -> PyObjectRef {
    // Build Path as a proper class with class methods (home, cwd) + constructor
    let mut path_ns = IndexMap::new();
    path_ns.insert(CompactString::from("home"), make_builtin(pathlib_home));
    path_ns.insert(CompactString::from("cwd"), make_builtin(pathlib_cwd));

    // exists() -> bool
    path_ns.insert(CompactString::from("exists"), make_builtin(|args| {
        if args.is_empty() { return Ok(PyObject::bool_val(false)); }
        Ok(PyObject::bool_val(std::path::Path::new(&get_path_str(&args[0])).exists()))
    }));

    // is_dir() -> bool
    path_ns.insert(CompactString::from("is_dir"), make_builtin(|args| {
        if args.is_empty() { return Ok(PyObject::bool_val(false)); }
        Ok(PyObject::bool_val(std::path::Path::new(&get_path_str(&args[0])).is_dir()))
    }));

    // is_file() -> bool
    path_ns.insert(CompactString::from("is_file"), make_builtin(|args| {
        if args.is_empty() { return Ok(PyObject::bool_val(false)); }
        Ok(PyObject::bool_val(std::path::Path::new(&get_path_str(&args[0])).is_file()))
    }));

    // mkdir(parents=False, exist_ok=False)
    path_ns.insert(CompactString::from("mkdir"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("mkdir requires self")); }
        let path = get_path_str(&args[0]);
        let mut parents = false;
        let mut exist_ok = false;
        for a in &args[1..] {
            if let PyObjectPayload::Dict(m) = &a.payload {
                let m = m.read();
                if let Some(v) = m.get(&HashableKey::Str(CompactString::from("parents"))) {
                    parents = v.is_truthy();
                }
                if let Some(v) = m.get(&HashableKey::Str(CompactString::from("exist_ok"))) {
                    exist_ok = v.is_truthy();
                }
            }
        }
        let result = if parents {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::create_dir(&path)
        };
        match result {
            Ok(()) => Ok(PyObject::none()),
            Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
            Err(e) => Err(PyException::runtime_error(format!("{}: '{}'", e, path))),
        }
    }));

    // read_text() -> str
    path_ns.insert(CompactString::from("read_text"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("read_text requires self")); }
        let path = get_path_str(&args[0]);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
        Ok(PyObject::str_val(CompactString::from(&content)))
    }));

    // read_bytes() -> bytes
    path_ns.insert(CompactString::from("read_bytes"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("read_bytes requires self")); }
        let path = get_path_str(&args[0]);
        let content = std::fs::read(&path)
            .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
        Ok(PyObject::bytes(content))
    }));

    // write_text(data) -> int
    path_ns.insert(CompactString::from("write_text"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("write_text requires self and data")); }
        let path = get_path_str(&args[0]);
        let text = args[1].py_to_string();
        let len = text.len();
        std::fs::write(&path, &text)
            .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
        Ok(PyObject::int(len as i64))
    }));

    // iterdir() -> list of Path instances
    path_ns.insert(CompactString::from("iterdir"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("iterdir requires self")); }
        let path = get_path_str(&args[0]);
        let entries = std::fs::read_dir(&path)
            .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
        let mut items = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path().to_string_lossy().to_string();
            items.push(make_path_instance(&p)?);
        }
        Ok(PyObject::list(items))
    }));

    // glob(pattern) -> list of Path instances
    path_ns.insert(CompactString::from("glob"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("glob requires self and pattern")); }
        let base = get_path_str(&args[0]);
        let pattern = args[1].py_to_string();
        let dir = std::path::Path::new(&base);
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if simple_glob_match(&pattern, &name) {
                    let full = entry.path().to_string_lossy().to_string();
                    results.push(make_path_instance(&full)?);
                }
            }
        }
        Ok(PyObject::list(results))
    }));

    // resolve() -> Path (absolute path)
    path_ns.insert(CompactString::from("resolve"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("resolve requires self")); }
        let path = get_path_str(&args[0]);
        let resolved = std::fs::canonicalize(&path)
            .unwrap_or_else(|_| std::path::PathBuf::from(&path));
        make_path_instance(&resolved.to_string_lossy())
    }));

    // unlink()
    path_ns.insert(CompactString::from("unlink"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("unlink requires self")); }
        let path = get_path_str(&args[0]);
        std::fs::remove_file(&path)
            .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
        Ok(PyObject::none())
    }));

    // __truediv__(other) -> Path  (the / operator)
    path_ns.insert(CompactString::from("__truediv__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("__truediv__ requires self and other")); }
        let base = get_path_str(&args[0]);
        let child = args[1].py_to_string();
        let joined = std::path::Path::new(&base).join(&child);
        make_path_instance(&joined.to_string_lossy())
    }));

    // __str__() -> str
    path_ns.insert(CompactString::from("__str__"), make_builtin(|args| {
        if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("."))); }
        Ok(PyObject::str_val(CompactString::from(get_path_str(&args[0]))))
    }));

    // __repr__() -> str
    path_ns.insert(CompactString::from("__repr__"), make_builtin(|args| {
        if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("PosixPath('.')"))); }
        let path = get_path_str(&args[0]);
        Ok(PyObject::str_val(CompactString::from(format!("PosixPath('{}')", path))))
    }));

    // __eq__(other) -> bool
    path_ns.insert(CompactString::from("__eq__"), make_builtin(|args| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a = get_path_str(&args[0]);
        let b = get_path_str(&args[1]);
        Ok(PyObject::bool_val(a == b))
    }));

    // __fspath__() -> str
    path_ns.insert(CompactString::from("__fspath__"), make_builtin(|args| {
        if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("."))); }
        Ok(PyObject::str_val(CompactString::from(get_path_str(&args[0]))))
    }));

    let path_cls = PyObject::class(CompactString::from("Path"), vec![], path_ns);
    // Add __init__ for constructor dispatch: Path("/some/path")
    if let PyObjectPayload::Class(ref cd) = path_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // args[0] = self (instance), args[1..] = path components
                if args.is_empty() { return Ok(PyObject::none()); }
                let path_str = if args.len() < 2 { ".".to_string() } else { args[1].py_to_string() };
                populate_path_instance(&args[0], &path_str)?;
                Ok(PyObject::none())
            }),
        );
    }
    make_module("pathlib", vec![
        ("Path", path_cls.clone()),
        ("PurePath", path_cls.clone()),
        ("PurePosixPath", path_cls.clone()),
        ("PureWindowsPath", path_cls),
    ])
}

/// Simple glob pattern matching (for use in the class-level glob method).
fn simple_glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" { return true; }
    if !pattern.contains('*') && !pattern.contains('?') { return pattern == text; }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if let Some(idx) = text[pos..].find(part) {
            if i == 0 && idx != 0 { return false; }
            pos += idx + part.len();
        } else { return false; }
    }
    parts.last().map_or(true, |p| p.is_empty() || pos == text.len())
}

fn pathlib_home(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    make_path_instance(&home)
}

fn pathlib_cwd(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    make_path_instance(&cwd)
}

/// Create a standalone Path instance (for class methods like home/cwd and internal use)
fn make_path_instance(path_str: &str) -> PyResult<PyObjectRef> {
    let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    populate_path_instance(&inst, path_str)?;
    Ok(inst)
}

/// Populate an existing instance with all Path attributes
fn populate_path_instance(inst: &PyObjectRef, path_str: &str) -> PyResult<()> {
    let path = std::path::Path::new(path_str);
    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let (stem_val, suffixes_vec) = compute_stem_suffixes(&file_name);
    let parent_str = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let parts: Vec<PyObjectRef> = path.components()
        .map(|c| PyObject::str_val(CompactString::from(c.as_os_str().to_string_lossy().to_string())))
        .collect();

    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(path_str)));
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(&file_name)));
        attrs.insert(CompactString::from("stem"), PyObject::str_val(CompactString::from(&stem_val)));
        attrs.insert(CompactString::from("suffix"), PyObject::str_val(CompactString::from(
            suffixes_vec.last().cloned().unwrap_or_default()
        )));
        attrs.insert(CompactString::from("suffixes"), PyObject::list(
            suffixes_vec.iter().map(|s| PyObject::str_val(CompactString::from(s.as_str()))).collect()
        ));
        if parent_str.is_empty() || parent_str == path_str {
            attrs.insert(CompactString::from("parent"), PyObject::str_val(CompactString::from(&parent_str)));
        } else {
            let parent_path = make_path_instance(&parent_str)?;
            attrs.insert(CompactString::from("parent"), parent_path);
        }
        attrs.insert(CompactString::from("parts"), PyObject::tuple(parts));
        attrs.insert(CompactString::from("__pathlib_path__"), PyObject::bool_val(true));
    }
    Ok(())
}

fn compute_stem_suffixes(file_name: &str) -> (String, Vec<String>) {
    if file_name.starts_with('.') && !file_name[1..].contains('.') {
        (file_name.to_string(), vec![])
    } else {
        let parts: Vec<&str> = file_name.splitn(2, '.').collect();
        if parts.len() > 1 {
            let suffixes: Vec<String> = parts[1].split('.').map(|s| format!(".{}", s)).collect();
            let last_dot = file_name.rfind('.').unwrap_or(file_name.len());
            let py_stem = file_name[..last_dot].to_string();
            (py_stem, suffixes)
        } else {
            (file_name.to_string(), vec![])
        }
    }
}



// ── unittest module (basic) ──


pub fn create_shutil_module() -> PyObjectRef {
    make_module("shutil", vec![
        ("copy", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("copy requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            std::fs::copy(&src, &dst).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("copy2", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("copy2 requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            std::fs::copy(&src, &dst).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("rmtree", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("rmtree requires path")); }
            let path = args[0].py_to_string();
            std::fs::remove_dir_all(&path).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::none())
        })),
        ("move", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("move requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            std::fs::rename(&src, &dst).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("which", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            let name = args[0].py_to_string();
            if let Ok(path) = std::env::var("PATH") {
                for dir in path.split(':') {
                    let candidate = std::path::Path::new(dir).join(&name);
                    if candidate.exists() {
                        return Ok(PyObject::str_val(CompactString::from(candidate.to_string_lossy().to_string())));
                    }
                }
            }
            Ok(PyObject::none())
        })),
        ("disk_usage", make_builtin(|_| Ok(PyObject::none()))),
        ("get_terminal_size", make_builtin(|_| {
            Ok(PyObject::tuple(vec![PyObject::int(80), PyObject::int(24)]))
        })),
        ("copytree", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("copytree requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                std::fs::create_dir_all(dst)?;
                for entry in std::fs::read_dir(src)? {
                    let entry = entry?;
                    let ty = entry.file_type()?;
                    let dest_path = dst.join(entry.file_name());
                    if ty.is_dir() {
                        copy_dir_recursive(&entry.path(), &dest_path)?;
                    } else {
                        std::fs::copy(entry.path(), &dest_path)?;
                    }
                }
                Ok(())
            }
            copy_dir_recursive(std::path::Path::new(&src), std::path::Path::new(&dst))
                .map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("copyfileobj", make_builtin(|_args| {
            // Stub — real impl would need file object support
            Ok(PyObject::none())
        })),
        ("ignore_patterns", make_builtin(|_args| {
            // Returns a callable that returns a set of patterns to ignore
            Ok(PyObject::native_function("_ignore", |_| {
                Ok(PyObject::set(indexmap::IndexMap::new()))
            }))
        })),
    ])
}

// ── glob module ──


pub fn create_glob_module() -> PyObjectRef {
    make_module("glob", vec![
        ("glob", make_builtin(glob_glob)),
        ("iglob", make_builtin(glob_glob)),
    ])
}

fn glob_glob(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("glob requires a pattern"));
    }
    let pattern = args[0].py_to_string();
    // Basic glob: handle *, ?, but not **
    // Use std::fs for simple patterns
    let path = std::path::Path::new(&pattern);
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let file_pattern = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if glob_match(&file_pattern, &name) {
                let full = entry.path().to_string_lossy().to_string();
                results.push(PyObject::str_val(CompactString::from(full)));
            }
        }
    }
    Ok(PyObject::list(results))
}

pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" { return true; }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == text;
    }
    // Simple wildcard matching
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        // No *, just ? wildcards
        if pattern.len() != text.len() { return false; }
        return pattern.chars().zip(text.chars()).all(|(p, t)| p == '?' || p == t);
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if let Some(idx) = text[pos..].find(part) {
            if i == 0 && idx != 0 { return false; }
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    if !parts.last().unwrap_or(&"").is_empty() {
        return pos == text.len();
    }
    true
}

// ── tempfile module (basic) ──

use std::sync::atomic::{AtomicU64, Ordering};

static TMPFILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Shared write buffers for NamedTemporaryFile instances, keyed by path.
static TMPFILE_BUFFERS: std::sync::LazyLock<Mutex<IndexMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

fn named_temporary_file(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract keyword args (mode, suffix, prefix, delete)
    let mut mode = String::from("w");
    let mut suffix = String::from("");
    let mut delete = true;
    // Check for trailing Dict kwargs
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
                    "prefix" => { /* ignored for now */ },
                    "delete" => delete = v.is_truthy(),
                    _ => {}
                }
            }
        }
    }

    let n = TMPFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!("ferrython_ntf_{}{}{}", std::process::id(), n, suffix));
    let path_str = path.to_string_lossy().to_string();

    // Create the file on disk
    std::fs::File::create(&path).map_err(|e|
        PyException::runtime_error(format!("tempfile: {}", e)))?;

    // Register a write buffer
    TMPFILE_BUFFERS.lock().unwrap().insert(path_str.clone(), String::new());

    let ps = path_str.clone();
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path_str.clone())));
    attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode.clone())));
    attrs.insert(CompactString::from("_delete"), PyObject::bool_val(delete));
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

    // write method
    let ps_w = ps.clone();
    attrs.insert(CompactString::from("write"), PyObject::native_closure("write", move |a| {
        let text = if !a.is_empty() { a[0].py_to_string() } else { String::new() };
        let mut bufs = TMPFILE_BUFFERS.lock().unwrap();
        if let Some(buf) = bufs.get_mut(&ps_w) {
            buf.push_str(&text);
        }
        Ok(PyObject::int(text.len() as i64))
    }));

    // flush method — write buffer to disk
    let ps_f = ps.clone();
    attrs.insert(CompactString::from("flush"), PyObject::native_closure("flush", move |_| {
        let bufs = TMPFILE_BUFFERS.lock().unwrap();
        if let Some(buf) = bufs.get(&ps_f) {
            std::fs::write(&ps_f, buf).ok();
        }
        Ok(PyObject::none())
    }));

    // close — flush + mark closed
    let ps_c = ps.clone();
    attrs.insert(CompactString::from("close"), PyObject::native_closure("close", move |_| {
        let content = TMPFILE_BUFFERS.lock().unwrap().shift_remove(&ps_c).unwrap_or_default();
        std::fs::write(&ps_c, &content).ok();
        Ok(PyObject::none())
    }));

    // __enter__ returns self (arg[0] is self when _bind_methods is set)
    attrs.insert(CompactString::from("__enter__"), PyObject::native_function("__enter__", |args| {
        if !args.is_empty() { Ok(args[0].clone()) } else { Ok(PyObject::none()) }
    }));

    // __exit__ — flush and optionally delete
    let ps_e = ps.clone();
    let del_flag = delete;
    attrs.insert(CompactString::from("__exit__"), PyObject::native_closure("__exit__", move |_| {
        let content = TMPFILE_BUFFERS.lock().unwrap().shift_remove(&ps_e).unwrap_or_default();
        std::fs::write(&ps_e, &content).ok();
        if del_flag {
            std::fs::remove_file(&ps_e).ok();
        }
        Ok(PyObject::bool_val(false))
    }));

    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));

    Ok(PyObject::module_with_attrs(CompactString::from("_tempfile"), attrs))
}


pub fn create_tempfile_module() -> PyObjectRef {
    make_module("tempfile", vec![
        ("gettempdir", make_builtin(|_| {
            Ok(PyObject::str_val(CompactString::from(
                std::env::temp_dir().to_string_lossy().to_string()
            )))
        })),
        ("mkdtemp", make_builtin(|_| {
            let dir = std::env::temp_dir().join(format!("ferrython_tmp_{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok();
            Ok(PyObject::str_val(CompactString::from(dir.to_string_lossy().to_string())))
        })),
        ("NamedTemporaryFile", make_builtin(named_temporary_file)),
        ("TemporaryDirectory", make_builtin(|_| Ok(PyObject::none()))),
        ("mkstemp", make_builtin(|_| {
            let path = std::env::temp_dir().join(format!("ferrython_{}", std::process::id()));
            Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::str_val(CompactString::from(path.to_string_lossy().to_string()))]))
        })),
    ])
}

// ── fnmatch module ──


pub fn create_io_module() -> PyObjectRef {
    make_module("io", vec![
        ("StringIO", make_builtin(io_string_io)),
        ("BytesIO", make_builtin(io_bytes_io)),
        ("TextIOWrapper", make_builtin(|_| Ok(PyObject::none()))),
        ("BufferedReader", make_builtin(|_| Ok(PyObject::none()))),
        ("BufferedWriter", make_builtin(|_| Ok(PyObject::none()))),
        ("SEEK_SET", PyObject::int(0)),
        ("SEEK_CUR", PyObject::int(1)),
        ("SEEK_END", PyObject::int(2)),
        ("DEFAULT_BUFFER_SIZE", PyObject::int(8192)),
        ("open", make_builtin(|args| {
            // io.open is an alias for builtins.open
            if args.is_empty() { return Err(PyException::type_error("open() requires at least 1 argument")); }
            Err(PyException::not_implemented_error("io.open() — use builtins.open()"))
        })),
    ])
}

/// Build a StringIO instance with methods attached to its instance dict.
fn io_string_io(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let initial = if args.is_empty() { String::new() } else { args[0].py_to_string() };
    let cls = PyObject::class(CompactString::from("StringIO"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__stringio__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));

        let buf: Arc<RwLock<String>> = Arc::new(RwLock::new(initial));
        let pos: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

        // write(s) → int
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("write"), PyObject::native_closure("StringIO.write", move |a: &[PyObjectRef]| {
            if a.is_empty() { return Err(PyException::type_error("write() takes 1 argument")); }
            let s = a[0].py_to_string();
            let len = s.len();
            let mut bw = b.write(); let mut pw = p.write();
            let cur = *pw;
            if cur >= bw.len() {
                bw.push_str(&s);
            } else {
                let end = cur + len;
                if end <= bw.len() {
                    bw.replace_range(cur..end, &s);
                } else {
                    bw.truncate(cur);
                    bw.push_str(&s);
                }
            }
            *pw = cur + len;
            Ok(PyObject::int(len as i64))
        }));

        // read(size=-1) → str
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("read"), PyObject::native_closure("StringIO.read", move |a: &[PyObjectRef]| {
            let size = if a.is_empty() { -1i64 } else { a[0].as_int().unwrap_or(-1) };
            let br = b.read(); let mut pw = p.write();
            let cur = *pw;
            if cur >= br.len() { return Ok(PyObject::str_val(CompactString::from(""))); }
            let end = if size < 0 { br.len() } else { (cur + size as usize).min(br.len()) };
            let result = &br[cur..end];
            *pw = end;
            Ok(PyObject::str_val(CompactString::from(result)))
        }));

        // getvalue() → str
        let b = buf.clone();
        attrs.insert(CompactString::from("getvalue"), PyObject::native_closure("StringIO.getvalue", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(b.read().as_str())))
        }));

        // seek(offset, whence=0) → int
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("seek"), PyObject::native_closure("StringIO.seek", move |a: &[PyObjectRef]| {
            if a.is_empty() { return Err(PyException::type_error("seek() takes at least 1 argument")); }
            let offset = a[0].as_int().unwrap_or(0);
            let whence = if a.len() > 1 { a[1].as_int().unwrap_or(0) } else { 0 };
            let br = b.read();
            let mut pw = p.write();
            let new_pos = match whence {
                0 => offset.max(0) as usize,
                1 => ((*pw as i64) + offset).max(0) as usize,
                2 => ((br.len() as i64) + offset).max(0) as usize,
                _ => return Err(PyException::value_error("invalid whence")),
            };
            *pw = new_pos;
            Ok(PyObject::int(new_pos as i64))
        }));

        // tell() → int
        let p = pos.clone();
        attrs.insert(CompactString::from("tell"), PyObject::native_closure("StringIO.tell", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(*p.read() as i64))
        }));

        // truncate(size=None) → int
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("truncate"), PyObject::native_closure("StringIO.truncate", move |a: &[PyObjectRef]| {
            let mut bw = b.write();
            let size = if a.is_empty() || matches!(&a[0].payload, PyObjectPayload::None) {
                *p.read()
            } else {
                a[0].as_int().unwrap_or(0) as usize
            };
            bw.truncate(size);
            Ok(PyObject::int(size as i64))
        }));

        // readline() → str
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("readline"), PyObject::native_closure("StringIO.readline", move |_: &[PyObjectRef]| {
            let br = b.read(); let mut pw = p.write();
            let cur = *pw;
            if cur >= br.len() { return Ok(PyObject::str_val(CompactString::from(""))); }
            let rest = &br[cur..];
            let end = rest.find('\n').map(|i| cur + i + 1).unwrap_or(br.len());
            *pw = end;
            Ok(PyObject::str_val(CompactString::from(&br[cur..end])))
        }));

        // readlines() → list[str]
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("readlines"), PyObject::native_closure("StringIO.readlines", move |_: &[PyObjectRef]| {
            let br = b.read(); let mut pw = p.write();
            let cur = *pw;
            if cur >= br.len() { return Ok(PyObject::list(vec![])); }
            let rest = &br[cur..];
            let lines: Vec<PyObjectRef> = rest.split_inclusive('\n')
                .map(|line| PyObject::str_val(CompactString::from(line)))
                .collect();
            *pw = br.len();
            Ok(PyObject::list(lines))
        }));

        // close()
        attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));

        // closed property (always False for simplicity)
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // __enter__ / __exit__ for context manager
        let inst_ref = inst.clone();
        attrs.insert(CompactString::from("__enter__"), PyObject::native_closure("StringIO.__enter__", move |_: &[PyObjectRef]| {
            Ok(inst_ref.clone())
        }));
        attrs.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::bool_val(false))));
    }
    Ok(inst)
}

/// Build a BytesIO instance with methods attached.
fn io_bytes_io(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let initial = if args.is_empty() {
        vec![]
    } else if let PyObjectPayload::Bytes(b) = &args[0].payload {
        b.clone()
    } else {
        vec![]
    };
    let cls = PyObject::class(CompactString::from("BytesIO"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__bytesio__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));

        let buf: Arc<RwLock<Vec<u8>>> = Arc::new(RwLock::new(initial));
        let pos: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

        // write(b) → int
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("write"), PyObject::native_closure("BytesIO.write", move |a: &[PyObjectRef]| {
            if a.is_empty() { return Err(PyException::type_error("write() takes 1 argument")); }
            let data = match &a[0].payload {
                PyObjectPayload::Bytes(v) => v.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
            };
            let len = data.len();
            let mut bw = b.write(); let mut pw = p.write();
            let cur = *pw;
            if cur >= bw.len() {
                bw.extend_from_slice(&data);
            } else {
                let end = cur + len;
                if end <= bw.len() {
                    bw[cur..end].copy_from_slice(&data);
                } else {
                    bw.truncate(cur);
                    bw.extend_from_slice(&data);
                }
            }
            *pw = cur + len;
            Ok(PyObject::int(len as i64))
        }));

        // read(size=-1) → bytes
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("read"), PyObject::native_closure("BytesIO.read", move |a: &[PyObjectRef]| {
            let size = if a.is_empty() { -1i64 } else { a[0].as_int().unwrap_or(-1) };
            let br = b.read(); let mut pw = p.write();
            let cur = *pw;
            if cur >= br.len() { return Ok(PyObject::bytes(vec![])); }
            let end = if size < 0 { br.len() } else { (cur + size as usize).min(br.len()) };
            let result = br[cur..end].to_vec();
            *pw = end;
            Ok(PyObject::bytes(result))
        }));

        // getvalue() → bytes
        let b = buf.clone();
        attrs.insert(CompactString::from("getvalue"), PyObject::native_closure("BytesIO.getvalue", move |_: &[PyObjectRef]| {
            Ok(PyObject::bytes(b.read().clone()))
        }));

        // seek(offset, whence=0) → int
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("seek"), PyObject::native_closure("BytesIO.seek", move |a: &[PyObjectRef]| {
            if a.is_empty() { return Err(PyException::type_error("seek() takes at least 1 argument")); }
            let offset = a[0].as_int().unwrap_or(0);
            let whence = if a.len() > 1 { a[1].as_int().unwrap_or(0) } else { 0 };
            let br = b.read();
            let mut pw = p.write();
            let new_pos = match whence {
                0 => offset.max(0) as usize,
                1 => ((*pw as i64) + offset).max(0) as usize,
                2 => ((br.len() as i64) + offset).max(0) as usize,
                _ => return Err(PyException::value_error("invalid whence")),
            };
            *pw = new_pos;
            Ok(PyObject::int(new_pos as i64))
        }));

        // tell() → int
        let p = pos.clone();
        attrs.insert(CompactString::from("tell"), PyObject::native_closure("BytesIO.tell", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(*p.read() as i64))
        }));

        // truncate(size=None) → int
        let b = buf.clone(); let p = pos.clone();
        attrs.insert(CompactString::from("truncate"), PyObject::native_closure("BytesIO.truncate", move |a: &[PyObjectRef]| {
            let mut bw = b.write();
            let size = if a.is_empty() || matches!(&a[0].payload, PyObjectPayload::None) {
                *p.read()
            } else {
                a[0].as_int().unwrap_or(0) as usize
            };
            bw.truncate(size);
            Ok(PyObject::int(size as i64))
        }));

        // close()
        attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // __enter__ / __exit__
        let inst_ref = inst.clone();
        attrs.insert(CompactString::from("__enter__"), PyObject::native_closure("BytesIO.__enter__", move |_: &[PyObjectRef]| {
            Ok(inst_ref.clone())
        }));
        attrs.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::bool_val(false))));
    }
    Ok(inst)
}


pub fn create_subprocess_module() -> PyObjectRef {
    make_module("subprocess", vec![
        ("PIPE", PyObject::int(-1)),
        ("STDOUT", PyObject::int(-2)),
        ("DEVNULL", PyObject::int(-3)),
        ("CalledProcessError", make_builtin(|_| Ok(PyObject::none()))),
        ("run", make_builtin(subprocess_run)),
        ("call", make_builtin(subprocess_call)),
        ("check_output", make_builtin(subprocess_check_output)),
        ("check_call", make_builtin(subprocess_call)),
        ("Popen", make_builtin(|_| {
            Err(PyException::runtime_error("subprocess.Popen not implemented"))
        })),
    ])
}

fn subprocess_run(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("subprocess.run requires arguments"));
    }
    let cmd_parts: Vec<String> = args[0].to_list()?.iter().map(|a| a.py_to_string()).collect();
    if cmd_parts.is_empty() {
        return Err(PyException::value_error("empty command"));
    }

    // Parse kwargs (last arg may be dict from VM kwarg passing)
    let mut text_mode = false;
    let mut _capture = false;
    let mut cwd: Option<String> = None;
    let mut shell = false;
    for arg in &args[1..] {
        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
            let r = kw_map.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("text"))) {
                text_mode = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("capture_output"))) {
                _capture = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("cwd"))) {
                cwd = Some(v.py_to_string());
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("shell"))) {
                shell = v.is_truthy();
            }
        }
    }

    let mut cmd = if shell {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd_parts.join(" "));
        c
    } else {
        let mut c = std::process::Command::new(&cmd_parts[0]);
        c.args(&cmd_parts[1..]);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output();
    match output {
        Ok(out) => {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("returncode"), PyObject::int(out.status.code().unwrap_or(-1) as i64));
            // If text=True, decode stdout/stderr as UTF-8 strings
            if text_mode {
                ns.insert(CompactString::from("stdout"),
                    PyObject::str_val(CompactString::from(String::from_utf8_lossy(&out.stdout).as_ref())));
                ns.insert(CompactString::from("stderr"),
                    PyObject::str_val(CompactString::from(String::from_utf8_lossy(&out.stderr).as_ref())));
            } else {
                ns.insert(CompactString::from("stdout"), PyObject::bytes(out.stdout));
                ns.insert(CompactString::from("stderr"), PyObject::bytes(out.stderr));
            }
            let cls = PyObject::class(CompactString::from("CompletedProcess"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(inst_data) = &inst.payload {
                let mut attrs = inst_data.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst)
        }
        Err(e) => Err(PyException::runtime_error(format!("subprocess error: {}", e))),
    }
}

fn subprocess_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    if let Some(rc) = result.get_attr("returncode") {
        Ok(rc)
    } else {
        Ok(PyObject::int(0))
    }
}

fn subprocess_check_output(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    if let Some(stdout) = result.get_attr("stdout") {
        Ok(stdout)
    } else {
        Ok(PyObject::bytes(vec![]))
    }
}

// ── byte extraction helper (used by zlib) ──

fn gzip_extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

// ── pathlib module (basic) ──



// ── zlib module ──

pub fn create_zlib_module() -> PyObjectRef {
    // zlib compress/decompress using DEFLATE stored blocks (same as gzip internals)
    let compress_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("zlib.compress requires data argument"));
        }
        let data = gzip_extract_bytes(&args[0])?;
        // zlib format: 2-byte header + deflate data + 4-byte adler32
        let mut out = Vec::with_capacity(6 + data.len() + 5);
        // CMF = 0x78 (deflate, window 32K), FLG = 0x01 (no dict, level 0)
        out.push(0x78);
        out.push(0x01);
        // DEFLATE stored blocks
        let mut offset = 0;
        while offset < data.len() {
            let remaining = data.len() - offset;
            let block_len = remaining.min(65535);
            let is_final: u8 = if offset + block_len >= data.len() { 0x01 } else { 0x00 };
            out.push(is_final);
            let len16 = block_len as u16;
            out.extend_from_slice(&len16.to_le_bytes());
            out.extend_from_slice(&(!len16).to_le_bytes());
            out.extend_from_slice(&data[offset..offset + block_len]);
            offset += block_len;
        }
        if data.is_empty() {
            out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
        }
        // Adler-32 checksum
        let adler = zlib_adler32(&data);
        out.extend_from_slice(&adler.to_be_bytes());
        Ok(PyObject::bytes(out))
    });

    let decompress_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("zlib.decompress requires data argument"));
        }
        let data = gzip_extract_bytes(&args[0])?;
        if data.len() < 6 {
            return Err(PyException::runtime_error("zlib.decompress: incomplete data"));
        }
        // Skip 2-byte zlib header
        let deflate_data = &data[2..data.len()-4]; // skip header and adler32 trailer
        let result = deflate_decompress_stored(deflate_data)?;
        Ok(PyObject::bytes(result))
    });

    let crc32_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("zlib.crc32 requires data argument"));
        }
        let data = gzip_extract_bytes(&args[0])?;
        let init = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as u32,
                _ => 0,
            }
        } else { 0 };
        let crc = gzip_crc32_with_init(&data, init);
        Ok(PyObject::int(crc as i64))
    });

    let adler32_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("zlib.adler32 requires data argument"));
        }
        let data = gzip_extract_bytes(&args[0])?;
        let adler = zlib_adler32(&data);
        Ok(PyObject::int(adler as i64))
    });

    make_module("zlib", vec![
        ("compress", compress_fn),
        ("decompress", decompress_fn),
        ("crc32", crc32_fn),
        ("adler32", adler32_fn),
        ("DEFLATED", PyObject::int(8)),
        ("MAX_WBITS", PyObject::int(15)),
        ("DEF_MEM_LEVEL", PyObject::int(8)),
        ("DEF_BUF_SIZE", PyObject::int(16384)),
        ("Z_DEFAULT_COMPRESSION", PyObject::int(-1)),
        ("Z_NO_COMPRESSION", PyObject::int(0)),
        ("Z_BEST_SPEED", PyObject::int(1)),
        ("Z_BEST_COMPRESSION", PyObject::int(9)),
    ])
}

fn zlib_adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn gzip_crc32_with_init(data: &[u8], init: u32) -> u32 {
    let mut crc = !init;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn deflate_decompress_stored(data: &[u8]) -> PyResult<Vec<u8>> {
    let mut result = Vec::new();
    let mut pos = 0;
    loop {
        if pos >= data.len() { break; }
        let bfinal = data[pos] & 1;
        let btype = (data[pos] >> 1) & 3;
        pos += 1;
        if btype == 0 {
            // Stored block
            if pos + 4 > data.len() { break; }
            let len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
            pos += 4; // skip len and nlen
            if pos + len > data.len() {
                result.extend_from_slice(&data[pos..]);
                break;
            }
            result.extend_from_slice(&data[pos..pos+len]);
            pos += len;
        } else {
            return Err(PyException::runtime_error("zlib.decompress: compressed data not supported (only stored blocks)"));
        }
        if bfinal != 0 { break; }
    }
    Ok(result)
}
