//! Filesystem and process stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use indexmap::IndexMap;

pub fn create_pathlib_module() -> PyObjectRef {
    make_module("pathlib", vec![
        ("Path", make_builtin(pathlib_path)),
        ("PurePath", make_builtin(pathlib_path)),
        ("PurePosixPath", make_builtin(pathlib_path)),
        ("PureWindowsPath", make_builtin(pathlib_path)),
    ])
}

fn pathlib_path(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path_str = if args.is_empty() { ".".to_string() } else { args[0].py_to_string() };
    let path = std::path::Path::new(&path_str);
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(path_str.as_str())));
    ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    )));
    // Python's Path.stem — everything before the last suffix (e.g. "test.tar" for "test.tar.gz")
    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let (stem_val, suffixes_vec) = if file_name.starts_with('.') && !file_name[1..].contains('.') {
        // Dotfile like ".gitignore" — stem is the whole name, no suffix
        (file_name.clone(), vec![])
    } else {
        let parts: Vec<&str> = file_name.splitn(2, '.').collect();
        if parts.len() > 1 {
            let _stem = parts[0].to_string();
            let suffixes: Vec<String> = parts[1].split('.').map(|s| format!(".{}", s)).collect();
            // Python stem = everything before the LAST dot suffix
            let last_dot = file_name.rfind('.').unwrap_or(file_name.len());
            let py_stem = file_name[..last_dot].to_string();
            (py_stem, suffixes)
        } else {
            (file_name.clone(), vec![])
        }
    };
    ns.insert(CompactString::from("stem"), PyObject::str_val(CompactString::from(&stem_val)));
    ns.insert(CompactString::from("suffix"), PyObject::str_val(CompactString::from(
        suffixes_vec.last().cloned().unwrap_or_default()
    )));
    ns.insert(CompactString::from("suffixes"), PyObject::list(
        suffixes_vec.iter().map(|s| PyObject::str_val(CompactString::from(s.as_str()))).collect()
    ));
    ns.insert(CompactString::from("parent"), PyObject::str_val(CompactString::from(
        path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    )));
    // parts — tuple of path components
    let parts: Vec<PyObjectRef> = path.components()
        .map(|c| PyObject::str_val(CompactString::from(c.as_os_str().to_string_lossy().to_string())))
        .collect();
    ns.insert(CompactString::from("parts"), PyObject::tuple(parts));
    // Methods that need the path are implemented via BuiltinBoundMethod in the VM
    ns.insert(CompactString::from("__pathlib_path__"), PyObject::bool_val(true));

    let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns { attrs.insert(k, v); }
    }
    Ok(inst)
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
        ("NamedTemporaryFile", make_builtin(|_| Ok(PyObject::none()))),
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
    ])
}

fn io_string_io(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let initial = if args.is_empty() { String::new() } else { args[0].py_to_string() };
    let cls = PyObject::class(CompactString::from("StringIO"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__stringio__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_buffer"), PyObject::str_val(CompactString::from(&initial)));
        attrs.insert(CompactString::from("_pos"), PyObject::int(initial.len() as i64));
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));
    }
    Ok(inst)
}

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
        attrs.insert(CompactString::from("_buffer"), PyObject::bytes(initial.clone()));
        attrs.insert(CompactString::from("_pos"), PyObject::int(initial.len() as i64));
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));
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
    let output = std::process::Command::new(&cmd_parts[0])
        .args(&cmd_parts[1..])
        .output();
    match output {
        Ok(out) => {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("returncode"), PyObject::int(out.status.code().unwrap_or(-1) as i64));
            ns.insert(CompactString::from("stdout"), PyObject::bytes(out.stdout));
            ns.insert(CompactString::from("stderr"), PyObject::bytes(out.stderr));
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

// ── pathlib module (basic) ──


