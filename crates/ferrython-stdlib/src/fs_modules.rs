//! Filesystem and process stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

/// Global Path class reference so helper functions can create proper Path instances.
static PATH_CLASS: OnceLock<PyObjectRef> = OnceLock::new();

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
    path_ns.insert(
        CompactString::from("exists"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(
                std::path::Path::new(&get_path_str(&args[0])).exists(),
            ))
        }),
    );

    // is_dir() -> bool
    path_ns.insert(
        CompactString::from("is_dir"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(
                std::path::Path::new(&get_path_str(&args[0])).is_dir(),
            ))
        }),
    );

    // is_file() -> bool
    path_ns.insert(
        CompactString::from("is_file"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(
                std::path::Path::new(&get_path_str(&args[0])).is_file(),
            ))
        }),
    );

    // mkdir(parents=False, exist_ok=False)
    path_ns.insert(
        CompactString::from("mkdir"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("mkdir requires self"));
            }
            let path = get_path_str(&args[0]);
            let mut parents = false;
            let mut exist_ok = false;
            for a in &args[1..] {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    let m = m.read();
                    if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("parents"))) {
                        parents = v.is_truthy();
                    }
                    if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("exist_ok"))) {
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
                Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => {
                    Ok(PyObject::none())
                }
                Err(e) => Err(PyException::from_io_error(&e, Some(&path))),
            }
        }),
    );

    // read_text() -> str
    path_ns.insert(
        CompactString::from("read_text"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("read_text requires self"));
            }
            let path = get_path_str(&args[0]);
            let content = std::fs::read_to_string(&path)
                .map_err(|e| PyException::from_io_error(&e, Some(&path)))?;
            Ok(PyObject::str_val(CompactString::from(&content)))
        }),
    );

    // read_bytes() -> bytes
    path_ns.insert(
        CompactString::from("read_bytes"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("read_bytes requires self"));
            }
            let path = get_path_str(&args[0]);
            let content = std::fs::read(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::bytes(content))
        }),
    );

    // write_text(data) -> int
    path_ns.insert(
        CompactString::from("write_text"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("write_text requires self and data"));
            }
            let path = get_path_str(&args[0]);
            let text = args[1].py_to_string();
            let len = text.len();
            std::fs::write(&path, &text)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::int(len as i64))
        }),
    );

    // iterdir() -> list of Path instances
    path_ns.insert(
        CompactString::from("iterdir"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("iterdir requires self"));
            }
            let path = get_path_str(&args[0]);
            let entries = std::fs::read_dir(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            let mut items = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path().to_string_lossy().to_string();
                items.push(make_path_instance(&p)?);
            }
            Ok(PyObject::list(items))
        }),
    );

    // glob(pattern) -> list of Path instances
    path_ns.insert(
        CompactString::from("glob"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("glob requires self and pattern"));
            }
            let base = get_path_str(&args[0]);
            let pattern = args[1].py_to_string();
            let dir = std::path::Path::new(&base);
            let mut results = Vec::new();
            if pattern.contains("**") {
                // Recursive glob: split on ** and match
                let suffix = pattern.trim_start_matches("**/").trim_start_matches("**");
                fn walk_glob(dir: &std::path::Path, suffix: &str, results: &mut Vec<PyObjectRef>) {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if simple_glob_match(suffix, &name) {
                                let full = entry.path().to_string_lossy().to_string();
                                if let Ok(p) = make_path_instance(&full) {
                                    results.push(p);
                                }
                            }
                            if entry.path().is_dir() {
                                walk_glob(&entry.path(), suffix, results);
                            }
                        }
                    }
                }
                walk_glob(dir, suffix, &mut results);
            } else {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if simple_glob_match(&pattern, &name) {
                            let full = entry.path().to_string_lossy().to_string();
                            results.push(make_path_instance(&full)?);
                        }
                    }
                }
            }
            Ok(PyObject::list(results))
        }),
    );

    // rglob(pattern) -> list of Path recursively matching
    path_ns.insert(
        CompactString::from("rglob"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("rglob requires self and pattern"));
            }
            let base = get_path_str(&args[0]);
            let pattern = args[1].py_to_string();
            let mut results = Vec::new();
            fn walk_rglob(dir: &std::path::Path, pattern: &str, results: &mut Vec<PyObjectRef>) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if simple_glob_match(pattern, &name) {
                            let full = entry.path().to_string_lossy().to_string();
                            if let Ok(p) = make_path_instance(&full) {
                                results.push(p);
                            }
                        }
                        if entry.path().is_dir() {
                            walk_rglob(&entry.path(), pattern, results);
                        }
                    }
                }
            }
            walk_rglob(std::path::Path::new(&base), &pattern, &mut results);
            Ok(PyObject::list(results))
        }),
    );

    // resolve() -> Path (absolute path)
    path_ns.insert(
        CompactString::from("resolve"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("resolve requires self"));
            }
            let path = get_path_str(&args[0]);
            let resolved =
                std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::PathBuf::from(&path));
            make_path_instance(&resolved.to_string_lossy())
        }),
    );

    // unlink()
    path_ns.insert(
        CompactString::from("unlink"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("unlink requires self"));
            }
            let path = get_path_str(&args[0]);
            std::fs::remove_file(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }),
    );

    // rmdir()
    path_ns.insert(
        CompactString::from("rmdir"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("rmdir requires self"));
            }
            let path = get_path_str(&args[0]);
            std::fs::remove_dir(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }),
    );

    // touch(exist_ok=True) — create empty file
    path_ns.insert(
        CompactString::from("touch"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("touch requires self"));
            }
            let path = get_path_str(&args[0]);
            let p = std::path::Path::new(&path);
            if !p.exists() {
                std::fs::File::create(&path)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            }
            Ok(PyObject::none())
        }),
    );

    // rename(target) -> Path
    path_ns.insert(
        CompactString::from("rename"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("rename requires self and target"));
            }
            let src = get_path_str(&args[0]);
            let dst = args[1].py_to_string();
            std::fs::rename(&src, &dst).map_err(|e| {
                PyException::runtime_error(format!("{}: '{}' -> '{}'", e, src, dst))
            })?;
            make_path_instance(&dst)
        }),
    );

    // replace(target) -> Path (atomic rename, overwrites destination)
    path_ns.insert(
        CompactString::from("replace"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("replace requires self and target"));
            }
            let src = get_path_str(&args[0]);
            let dst = args[1].py_to_string();
            std::fs::rename(&src, &dst).map_err(|e| {
                PyException::runtime_error(format!("{}: '{}' -> '{}'", e, src, dst))
            })?;
            make_path_instance(&dst)
        }),
    );

    // is_relative_to(other) -> bool (Python 3.9+)
    path_ns.insert(
        CompactString::from("is_relative_to"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "is_relative_to requires self and other",
                ));
            }
            let path = get_path_str(&args[0]);
            let other = args[1].py_to_string();
            let p = std::path::Path::new(&path);
            let o = std::path::Path::new(&other);
            Ok(PyObject::bool_val(p.starts_with(o)))
        }),
    );

    // is_symlink() -> bool
    path_ns.insert(
        CompactString::from("is_symlink"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            let path = get_path_str(&args[0]);
            Ok(PyObject::bool_val(std::path::Path::new(&path).is_symlink()))
        }),
    );

    // stat() -> os.stat_result-like object
    path_ns.insert(
        CompactString::from("stat"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("stat requires self"));
            }
            let path = get_path_str(&args[0]);
            let meta = std::fs::metadata(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            build_stat_result(meta)
        }),
    );

    // with_name(name) -> Path
    path_ns.insert(
        CompactString::from("with_name"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("with_name requires self and name"));
            }
            let path = get_path_str(&args[0]);
            let new_name = args[1].py_to_string();
            let p = std::path::Path::new(&path);
            let parent = p.parent().unwrap_or(std::path::Path::new(""));
            let new_path = parent.join(&new_name);
            make_path_instance(&new_path.to_string_lossy())
        }),
    );

    // with_suffix(suffix) -> Path
    path_ns.insert(
        CompactString::from("with_suffix"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "with_suffix requires self and suffix",
                ));
            }
            let path = get_path_str(&args[0]);
            let new_suffix = args[1].py_to_string();
            let p = std::path::Path::new(&path);
            let new_path = p.with_extension(new_suffix.trim_start_matches('.'));
            make_path_instance(&new_path.to_string_lossy())
        }),
    );

    // open(mode='r') -> file-like object
    path_ns.insert(
        CompactString::from("open"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("open requires self"));
            }
            let path = get_path_str(&args[0]);
            let mode = if args.len() > 1 {
                args[1].py_to_string()
            } else {
                "r".to_string()
            };
            // Delegate to builtins.open logic — return file-like object
            let content = if mode.contains('r') {
                std::fs::read_to_string(&path)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?
            } else {
                String::new()
            };
            Ok(PyObject::str_val(CompactString::from(content)))
        }),
    );

    // __truediv__(other) -> Path  (the / operator)
    path_ns.insert(
        CompactString::from("__truediv__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "__truediv__ requires self and other",
                ));
            }
            let base = get_path_str(&args[0]);
            let child = args[1].py_to_string();
            let joined = std::path::Path::new(&base).join(&child);
            make_path_instance(&joined.to_string_lossy())
        }),
    );

    // __str__() -> str
    path_ns.insert(
        CompactString::from("__str__"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from(".")));
            }
            Ok(PyObject::str_val(CompactString::from(get_path_str(
                &args[0],
            ))))
        }),
    );

    // __repr__() -> str
    path_ns.insert(
        CompactString::from("__repr__"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("PosixPath('.')")));
            }
            let path = get_path_str(&args[0]);
            Ok(PyObject::str_val(CompactString::from(format!(
                "PosixPath('{}')",
                path
            ))))
        }),
    );

    // __eq__(other) -> bool
    path_ns.insert(
        CompactString::from("__eq__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a = get_path_str(&args[0]);
            let b = get_path_str(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }),
    );

    // __fspath__() -> str
    path_ns.insert(
        CompactString::from("__fspath__"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from(".")));
            }
            Ok(PyObject::str_val(CompactString::from(get_path_str(
                &args[0],
            ))))
        }),
    );

    // relative_to(other) -> Path
    path_ns.insert(
        CompactString::from("relative_to"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "relative_to requires self and other",
                ));
            }
            let path = get_path_str(&args[0]);
            let base = args[1].py_to_string();
            let p = std::path::Path::new(&path);
            let b = std::path::Path::new(&base);
            match p.strip_prefix(b) {
                Ok(rel) => make_path_instance(&rel.to_string_lossy()),
                Err(_) => Err(PyException::value_error(format!(
                    "'{}' is not relative to '{}'",
                    path, base
                ))),
            }
        }),
    );

    // with_stem(stem) -> Path (Python 3.9+)
    path_ns.insert(
        CompactString::from("with_stem"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("with_stem requires self and stem"));
            }
            let path = get_path_str(&args[0]);
            let new_stem = args[1].py_to_string();
            let p = std::path::Path::new(&path);
            let ext = p
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let parent = p.parent().unwrap_or(std::path::Path::new(""));
            let new_path = parent.join(format!("{}{}", new_stem, ext));
            make_path_instance(&new_path.to_string_lossy())
        }),
    );

    // expanduser() -> Path
    path_ns.insert(
        CompactString::from("expanduser"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("expanduser requires self"));
            }
            let path = get_path_str(&args[0]);
            if path.starts_with("~/") || path == "~" {
                if let Ok(home) = std::env::var("HOME") {
                    let expanded = if path == "~" {
                        home
                    } else {
                        format!("{}{}", home, &path[1..])
                    };
                    return make_path_instance(&expanded);
                }
            }
            make_path_instance(&path)
        }),
    );

    // is_absolute() -> bool
    path_ns.insert(
        CompactString::from("is_absolute"),
        make_builtin(|args| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(
                std::path::Path::new(&get_path_str(&args[0])).is_absolute(),
            ))
        }),
    );

    // absolute() -> Path (like resolve but without symlink resolution)
    path_ns.insert(
        CompactString::from("absolute"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("absolute requires self"));
            }
            let path = get_path_str(&args[0]);
            let p = std::path::Path::new(&path);
            if p.is_absolute() {
                make_path_instance(&path)
            } else {
                let cwd = std::env::current_dir().unwrap_or_default();
                make_path_instance(&cwd.join(p).to_string_lossy())
            }
        }),
    );

    // match(pattern) -> bool (simple glob match against the path name)
    path_ns.insert(
        CompactString::from("match"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let path = get_path_str(&args[0]);
            let pattern = args[1].py_to_string();
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Ok(PyObject::bool_val(simple_glob_match(&pattern, &name)))
        }),
    );

    // samefile(other) -> bool
    path_ns.insert(
        CompactString::from("samefile"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a = get_path_str(&args[0]);
            let b = args[1].py_to_string();
            let ma = std::fs::metadata(&a);
            let mb = std::fs::metadata(&b);
            match (ma, mb) {
                #[cfg(unix)]
                (Ok(ma), Ok(mb)) => {
                    use std::os::unix::fs::MetadataExt;
                    Ok(PyObject::bool_val(
                        ma.ino() == mb.ino() && ma.dev() == mb.dev(),
                    ))
                }
                #[cfg(not(unix))]
                (Ok(_), Ok(_)) => {
                    let ca = std::fs::canonicalize(&a).unwrap_or_default();
                    let cb = std::fs::canonicalize(&b).unwrap_or_default();
                    Ok(PyObject::bool_val(ca == cb))
                }
                _ => Ok(PyObject::bool_val(false)),
            }
        }),
    );

    // write_bytes(data) -> int
    path_ns.insert(
        CompactString::from("write_bytes"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "write_bytes requires self and data",
                ));
            }
            let path = get_path_str(&args[0]);
            let data = match &args[1].payload {
                PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                _ => args[1].py_to_string().into_bytes(),
            };
            let len = data.len();
            std::fs::write(&path, &data)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::int(len as i64))
        }),
    );

    // lstat() -> stat_result (without following symlinks)
    path_ns.insert(
        CompactString::from("lstat"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("lstat requires self"));
            }
            let path = get_path_str(&args[0]);
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            build_stat_result(meta)
        }),
    );

    // chmod(mode)
    #[cfg(unix)]
    path_ns.insert(
        CompactString::from("chmod"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("chmod requires self and mode"));
            }
            let path = get_path_str(&args[0]);
            let mode = args[1].as_int().unwrap_or(0o644) as u32;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }),
    );

    let path_cls = PyObject::class(CompactString::from("Path"), vec![], path_ns);
    // Store global ref so make_path_instance() can create proper Path objects
    let _ = PATH_CLASS.set(path_cls.clone());
    // Add __init__ for constructor dispatch: Path("/some/path", "subpath", ...)
    if let PyObjectPayload::Class(ref cd) = path_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // args[0] = self (instance), args[1..] = path components
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                let path_str = if args.len() < 2 {
                    ".".to_string()
                } else if args.len() == 2 {
                    args[1].py_to_string()
                } else {
                    // Join all path components like CPython: Path(a, b, c) -> a/b/c
                    let mut buf = std::path::PathBuf::from(args[1].py_to_string());
                    for arg in &args[2..] {
                        buf.push(arg.py_to_string());
                    }
                    buf.to_string_lossy().to_string()
                };
                populate_path_instance(&args[0], &path_str)?;
                Ok(PyObject::none())
            }),
        );
    }
    make_module(
        "pathlib",
        vec![
            ("Path", path_cls.clone()),
            ("PurePath", path_cls.clone()),
            ("PurePosixPath", path_cls.clone()),
            ("PureWindowsPath", path_cls),
        ],
    )
}

/// Simple glob pattern matching (for use in the class-level glob method).
fn simple_glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == text;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(idx) = text[pos..].find(part) {
            if i == 0 && idx != 0 {
                return false;
            }
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    parts
        .last()
        .map_or(true, |p| p.is_empty() || pos == text.len())
}

fn pathlib_home(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    make_path_instance(&home)
}

pub fn build_stat_result(meta: std::fs::Metadata) -> PyResult<PyObjectRef> {
    let cls = PyObject::class(CompactString::from("stat_result"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("st_size"),
            PyObject::int(meta.len() as i64),
        );
        w.insert(CompactString::from("st_mode"), PyObject::int(0o644));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            w.insert(
                CompactString::from("st_mode"),
                PyObject::int(meta.mode() as i64),
            );
            w.insert(
                CompactString::from("st_ino"),
                PyObject::int(meta.ino() as i64),
            );
            w.insert(
                CompactString::from("st_dev"),
                PyObject::int(meta.dev() as i64),
            );
            w.insert(
                CompactString::from("st_nlink"),
                PyObject::int(meta.nlink() as i64),
            );
            w.insert(
                CompactString::from("st_uid"),
                PyObject::int(meta.uid() as i64),
            );
            w.insert(
                CompactString::from("st_gid"),
                PyObject::int(meta.gid() as i64),
            );
            w.insert(
                CompactString::from("st_atime"),
                PyObject::float(meta.atime() as f64),
            );
            w.insert(
                CompactString::from("st_mtime"),
                PyObject::float(meta.mtime() as f64),
            );
            w.insert(
                CompactString::from("st_ctime"),
                PyObject::float(meta.ctime() as f64),
            );
        }
    }
    Ok(inst)
}

fn pathlib_cwd(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    make_path_instance(&cwd)
}

/// Create a standalone Path instance (for class methods like home/cwd and internal use)
fn make_path_instance(path_str: &str) -> PyResult<PyObjectRef> {
    let cls = PATH_CLASS
        .get()
        .cloned()
        .unwrap_or_else(|| PyObject::class(CompactString::from("Path"), vec![], IndexMap::new()));
    let inst = PyObject::instance(cls);
    populate_path_instance(&inst, path_str)?;
    Ok(inst)
}

/// Populate an existing instance with all Path attributes
fn populate_path_instance(inst: &PyObjectRef, path_str: &str) -> PyResult<()> {
    let path = std::path::Path::new(path_str);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let (stem_val, suffixes_vec) = compute_stem_suffixes(&file_name);
    let parent_str = path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let parts: Vec<PyObjectRef> = path
        .components()
        .map(|c| {
            PyObject::str_val(CompactString::from(
                c.as_os_str().to_string_lossy().to_string(),
            ))
        })
        .collect();

    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("_path"),
            PyObject::str_val(CompactString::from(path_str)),
        );
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(&file_name)),
        );
        attrs.insert(
            CompactString::from("stem"),
            PyObject::str_val(CompactString::from(&stem_val)),
        );
        attrs.insert(
            CompactString::from("suffix"),
            PyObject::str_val(CompactString::from(
                suffixes_vec.last().cloned().unwrap_or_default(),
            )),
        );
        attrs.insert(
            CompactString::from("suffixes"),
            PyObject::list(
                suffixes_vec
                    .iter()
                    .map(|s| PyObject::str_val(CompactString::from(s.as_str())))
                    .collect(),
            ),
        );
        if parent_str.is_empty() || parent_str == path_str {
            attrs.insert(
                CompactString::from("parent"),
                PyObject::str_val(CompactString::from(&parent_str)),
            );
        } else {
            let parent_path = make_path_instance(&parent_str)?;
            attrs.insert(CompactString::from("parent"), parent_path);
        }
        // parents — list of all ancestor Path objects from immediate parent to root
        let mut parents_list = Vec::new();
        let mut cur = path.parent();
        while let Some(p) = cur {
            let ps = p.to_string_lossy().to_string();
            if ps.is_empty() {
                break;
            }
            parents_list.push(make_path_instance(&ps)?);
            cur = p.parent();
            if Some(p) == cur {
                break;
            } // root reached
        }
        attrs.insert(CompactString::from("parents"), PyObject::list(parents_list));
        attrs.insert(CompactString::from("parts"), PyObject::tuple(parts));
        // root: "/" for absolute paths, "" for relative
        let root_str = if path_str.starts_with('/') { "/" } else { "" };
        attrs.insert(
            CompactString::from("root"),
            PyObject::str_val(CompactString::from(root_str)),
        );
        // anchor: same as root on POSIX (drive + root on Windows)
        attrs.insert(
            CompactString::from("anchor"),
            PyObject::str_val(CompactString::from(root_str)),
        );
        // drive: always empty on POSIX
        attrs.insert(
            CompactString::from("drive"),
            PyObject::str_val(CompactString::from("")),
        );
        attrs.insert(
            CompactString::from("__pathlib_path__"),
            PyObject::bool_val(true),
        );
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
    make_module(
        "shutil",
        vec![
            (
                "copy",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("copy requires src and dst"));
                    }
                    let src = args[0].py_to_string();
                    let mut dst = std::path::PathBuf::from(args[1].py_to_string());
                    if dst.is_dir() {
                        if let Some(fname) = std::path::Path::new(&src).file_name() {
                            dst = dst.join(fname);
                        }
                    }
                    std::fs::copy(&src, &dst)
                        .map_err(|e| PyException::runtime_error(format!("{}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(
                        dst.to_string_lossy().to_string(),
                    )))
                }),
            ),
            (
                "copy2",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("copy2 requires src and dst"));
                    }
                    let src = args[0].py_to_string();
                    let mut dst = std::path::PathBuf::from(args[1].py_to_string());
                    if dst.is_dir() {
                        if let Some(fname) = std::path::Path::new(&src).file_name() {
                            dst = dst.join(fname);
                        }
                    }
                    std::fs::copy(&src, &dst)
                        .map_err(|e| PyException::runtime_error(format!("{}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(
                        dst.to_string_lossy().to_string(),
                    )))
                }),
            ),
            (
                "rmtree",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("rmtree requires path"));
                    }
                    let path = args[0].py_to_string();
                    std::fs::remove_dir_all(&path)
                        .map_err(|e| PyException::runtime_error(format!("{}", e)))?;
                    Ok(PyObject::none())
                }),
            ),
            (
                "move",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("move requires src and dst"));
                    }
                    let src = args[0].py_to_string();
                    let mut dst = std::path::PathBuf::from(args[1].py_to_string());
                    if dst.is_dir() {
                        if let Some(fname) = std::path::Path::new(&src).file_name() {
                            dst = dst.join(fname);
                        }
                    }
                    std::fs::rename(&src, &dst)
                        .map_err(|e| PyException::runtime_error(format!("{}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(
                        dst.to_string_lossy().to_string(),
                    )))
                }),
            ),
            (
                "which",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    let name = args[0].py_to_string();
                    if let Ok(path) = std::env::var("PATH") {
                        for dir in path.split(':') {
                            let candidate = std::path::Path::new(dir).join(&name);
                            if candidate.exists() {
                                return Ok(PyObject::str_val(CompactString::from(
                                    candidate.to_string_lossy().to_string(),
                                )));
                            }
                        }
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "disk_usage",
                make_builtin(|args| {
                    let path = if args.is_empty() {
                        "/".to_string()
                    } else {
                        args[0].py_to_string()
                    };
                    let output = std::process::Command::new("df")
                        .arg("-k")
                        .arg(&path)
                        .output();
                    let (total, used, free) = if let Ok(out) = output {
                        let text = String::from_utf8_lossy(&out.stdout);
                        if let Some(line) = text.lines().nth(1) {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if parts.len() >= 4 {
                                (
                                    parts[1].parse::<i64>().unwrap_or(0) * 1024,
                                    parts[2].parse::<i64>().unwrap_or(0) * 1024,
                                    parts[3].parse::<i64>().unwrap_or(0) * 1024,
                                )
                            } else {
                                (0, 0, 0)
                            }
                        } else {
                            (0, 0, 0)
                        }
                    } else {
                        (0, 0, 0)
                    };
                    let cls =
                        PyObject::class(CompactString::from("usage"), vec![], IndexMap::new());
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("total"), PyObject::int(total));
                    attrs.insert(CompactString::from("used"), PyObject::int(used));
                    attrs.insert(CompactString::from("free"), PyObject::int(free));
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }),
            ),
            (
                "get_terminal_size",
                make_builtin(|_| {
                    let cols = std::env::var("COLUMNS")
                        .ok()
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(80);
                    let lines = std::env::var("LINES")
                        .ok()
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(24);
                    Ok(crate::sys_modules::make_terminal_size_instance(cols, lines))
                }),
            ),
            (
                "copytree",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("copytree requires src and dst"));
                    }
                    let src = args[0].py_to_string();
                    let dst = args[1].py_to_string();
                    fn copy_dir_recursive(
                        src: &std::path::Path,
                        dst: &std::path::Path,
                    ) -> std::io::Result<()> {
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
                }),
            ),
            ("copyfileobj", make_builtin(|_args| Ok(PyObject::none()))),
            (
                "copyfile",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("copyfile requires src and dst"));
                    }
                    let src = args[0].py_to_string();
                    let dst = args[1].py_to_string();
                    std::fs::copy(&src, &dst)
                        .map_err(|e| PyException::runtime_error(format!("{}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(dst)))
                }),
            ),
            (
                "copymode",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("copymode requires src and dst"));
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let src = args[0].py_to_string();
                        let dst = args[1].py_to_string();
                        if let Ok(meta) = std::fs::metadata(&src) {
                            let _ = std::fs::set_permissions(
                                &dst,
                                std::fs::Permissions::from_mode(meta.permissions().mode()),
                            );
                        }
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "copystat",
                make_builtin(|args: &[PyObjectRef]| {
                    // Copies metadata (mtime, atime, permissions) from src to dst
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "copystat() requires 2 arguments: src, dst",
                        ));
                    }
                    let src = args[0].py_to_string();
                    let dst = args[1].py_to_string();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        // Copy permissions
                        if let Ok(meta) = std::fs::metadata(&src) {
                            let perms = meta.permissions();
                            let _ = std::fs::set_permissions(
                                &dst,
                                std::fs::Permissions::from_mode(perms.mode()),
                            );
                            // Copy timestamps via libc::utimensat
                            use std::time::UNIX_EPOCH;
                            let atime = meta
                                .accessed()
                                .ok()
                                .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                            let mtime = meta
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                            if let (Some(at), Some(mt)) = (atime, mtime) {
                                let times = [
                                    libc::timespec {
                                        tv_sec: at.as_secs() as libc::time_t,
                                        tv_nsec: at.subsec_nanos() as libc::c_long,
                                    },
                                    libc::timespec {
                                        tv_sec: mt.as_secs() as libc::time_t,
                                        tv_nsec: mt.subsec_nanos() as libc::c_long,
                                    },
                                ];
                                let c_dst =
                                    std::ffi::CString::new(dst.as_str()).unwrap_or_default();
                                unsafe {
                                    libc::utimensat(
                                        libc::AT_FDCWD,
                                        c_dst.as_ptr(),
                                        times.as_ptr(),
                                        0,
                                    );
                                }
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (src, dst);
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "ignore_patterns",
                make_builtin(|args: &[PyObjectRef]| {
                    let patterns: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
                    Ok(PyObject::native_closure(
                        "_ignore_patterns",
                        move |inner_args: &[PyObjectRef]| {
                            // inner_args: (path, names)
                            let names = if inner_args.len() > 1 {
                                match &inner_args[1].payload {
                                    PyObjectPayload::List(items) => items
                                        .read()
                                        .iter()
                                        .map(|i| i.py_to_string())
                                        .collect::<Vec<_>>(),
                                    _ => vec![],
                                }
                            } else {
                                vec![]
                            };
                            let mut ignored = IndexMap::new();
                            for pattern in &patterns {
                                for name in &names {
                                    if glob_match(pattern, name) {
                                        ignored.insert(
                                            HashableKey::str_key(CompactString::from(
                                                name.as_str(),
                                            )),
                                            PyObject::str_val(CompactString::from(name.as_str())),
                                        );
                                    }
                                }
                            }
                            Ok(PyObject::set(ignored))
                        },
                    ))
                }),
            ),
            (
                "make_archive",
                make_builtin(|args| {
                    // make_archive(base_name, format, root_dir=None, base_dir=None)
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "make_archive requires base_name and format",
                        ));
                    }
                    let base_name = args[0].py_to_string();
                    let format = args[1].py_to_string();
                    let root_dir =
                        if args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::None) {
                            args[2].py_to_string()
                        } else {
                            ".".to_string()
                        };
                    let archive_name = match format.as_str() {
                        "zip" => format!("{}.zip", base_name),
                        "tar" => format!("{}.tar", base_name),
                        "gztar" => format!("{}.tar.gz", base_name),
                        "bztar" => format!("{}.tar.bz2", base_name),
                        "xztar" => format!("{}.tar.xz", base_name),
                        _ => {
                            return Err(PyException::value_error(format!(
                                "unknown archive format: {}",
                                format
                            )))
                        }
                    };
                    // Use tar/zip commands for real archiving
                    let cmd = match format.as_str() {
                        "zip" => format!(
                            "cd '{}' && zip -r '{}' .",
                            root_dir,
                            std::fs::canonicalize(&archive_name)
                                .unwrap_or(std::path::PathBuf::from(&archive_name))
                                .display()
                        ),
                        "tar" => format!("tar cf '{}' -C '{}' .", archive_name, root_dir),
                        "gztar" => format!("tar czf '{}' -C '{}' .", archive_name, root_dir),
                        _ => format!("tar cf '{}' -C '{}' .", archive_name, root_dir),
                    };
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .status()
                        .map_err(|e| PyException::runtime_error(format!("make_archive: {}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(&archive_name)))
                }),
            ),
            (
                "unpack_archive",
                make_builtin(|args| {
                    // unpack_archive(filename, extract_dir=None, format=None)
                    if args.is_empty() {
                        return Err(PyException::type_error("unpack_archive requires filename"));
                    }
                    let filename = args[0].py_to_string();
                    let extract_dir =
                        if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
                            args[1].py_to_string()
                        } else {
                            ".".to_string()
                        };
                    let cmd = if filename.ends_with(".zip") {
                        format!("unzip -o '{}' -d '{}'", filename, extract_dir)
                    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
                        format!("tar xzf '{}' -C '{}'", filename, extract_dir)
                    } else if filename.ends_with(".tar.bz2") {
                        format!("tar xjf '{}' -C '{}'", filename, extract_dir)
                    } else if filename.ends_with(".tar.xz") {
                        format!("tar xJf '{}' -C '{}'", filename, extract_dir)
                    } else if filename.ends_with(".tar") {
                        format!("tar xf '{}' -C '{}'", filename, extract_dir)
                    } else {
                        return Err(PyException::value_error(format!(
                            "unknown archive format: {}",
                            filename
                        )));
                    };
                    std::fs::create_dir_all(&extract_dir).ok();
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .status()
                        .map_err(|e| {
                            PyException::runtime_error(format!("unpack_archive: {}", e))
                        })?;
                    Ok(PyObject::none())
                }),
            ),
            (
                "get_archive_formats",
                make_builtin(|_| {
                    Ok(PyObject::list(vec![
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("zip")),
                            PyObject::str_val(CompactString::from("ZIP file")),
                        ]),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("tar")),
                            PyObject::str_val(CompactString::from("uncompressed tar file")),
                        ]),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("gztar")),
                            PyObject::str_val(CompactString::from("gzip'ed tar-file")),
                        ]),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("bztar")),
                            PyObject::str_val(CompactString::from("bzip2'ed tar-file")),
                        ]),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("xztar")),
                            PyObject::str_val(CompactString::from("xz'ed tar-file")),
                        ]),
                    ]))
                }),
            ),
            (
                "get_unpack_formats",
                make_builtin(|_| {
                    Ok(PyObject::list(vec![
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("zip")),
                            PyObject::list(vec![PyObject::str_val(CompactString::from(".zip"))]),
                            PyObject::str_val(CompactString::from("ZIP file")),
                        ]),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("tar")),
                            PyObject::list(vec![PyObject::str_val(CompactString::from(".tar"))]),
                            PyObject::str_val(CompactString::from("uncompressed tar file")),
                        ]),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("gztar")),
                            PyObject::list(vec![
                                PyObject::str_val(CompactString::from(".tar.gz")),
                                PyObject::str_val(CompactString::from(".tgz")),
                            ]),
                            PyObject::str_val(CompactString::from("gzip'ed tar-file")),
                        ]),
                    ]))
                }),
            ),
        ],
    )
}

// ── glob module ──

pub fn create_glob_module() -> PyObjectRef {
    make_module(
        "glob",
        vec![
            ("glob", make_builtin(glob_glob)),
            ("iglob", make_builtin(glob_glob)),
            ("escape", make_builtin(glob_escape)),
            (
                "has_magic",
                make_builtin(|args: &[PyObjectRef]| {
                    check_args("glob.has_magic", args, 1)?;
                    let s = args[0].py_to_string();
                    Ok(PyObject::bool_val(
                        s.contains('*') || s.contains('?') || s.contains('[') || s.contains(']'),
                    ))
                }),
            ),
        ],
    )
}

fn glob_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("escape requires a pathname"));
    }
    let s = args[0].py_to_string();
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '*' | '?' | '[' => {
                let mut r = String::from('[');
                r.push(c);
                r.push(']');
                r
            }
            _ => c.to_string(),
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(escaped)))
}

fn glob_glob(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("glob requires a pattern"));
    }
    let pattern = args[0].py_to_string();
    // Check for recursive kwarg
    let recursive = if args.len() > 1 {
        args[1].is_truthy()
    } else {
        pattern.contains("**")
    };

    let mut results = Vec::new();
    if recursive && pattern.contains("**") {
        glob_recursive(&pattern, &mut results)?;
    } else {
        glob_simple(&pattern, &mut results)?;
    }
    results.sort_by(|a, b| a.py_to_string().cmp(&b.py_to_string()));
    Ok(PyObject::list(results))
}

fn glob_simple(pattern: &str, results: &mut Vec<PyObjectRef>) -> PyResult<()> {
    glob_expand(pattern, results);
    Ok(())
}

/// Recursively expand glob pattern by handling wildcards in any path component.
fn glob_expand(pattern: &str, results: &mut Vec<PyObjectRef>) {
    // Split pattern into components
    let parts: Vec<&str> = pattern.split('/').collect();

    // Find first component with a wildcard
    let wild_idx = parts
        .iter()
        .position(|p| p.contains('*') || p.contains('?') || p.contains('['));

    match wild_idx {
        None => {
            // No wildcards: check if the literal path exists
            let p = std::path::Path::new(pattern);
            if p.exists() {
                results.push(PyObject::str_val(CompactString::from(pattern)));
            }
        }
        Some(idx) => {
            let dir_prefix: String = if idx == 0 {
                ".".to_string()
            } else {
                parts[..idx].join("/")
            };
            let wild_part = parts[idx];
            let rest: Option<String> = if idx + 1 < parts.len() {
                Some(parts[idx + 1..].join("/"))
            } else {
                None
            };

            if let Ok(entries) = std::fs::read_dir(&dir_prefix) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !glob_match(wild_part, &name) {
                        continue;
                    }

                    let matched_path = if idx == 0 {
                        name
                    } else {
                        format!("{}/{}", parts[..idx].join("/"), name)
                    };

                    match &rest {
                        None => {
                            results.push(PyObject::str_val(CompactString::from(matched_path)));
                        }
                        Some(remainder) => {
                            let sub = format!("{}/{}", matched_path, remainder);
                            glob_expand(&sub, results);
                        }
                    }
                }
            }
        }
    }
}

fn glob_recursive(pattern: &str, results: &mut Vec<PyObjectRef>) -> PyResult<()> {
    // Split on ** to get prefix and suffix
    // e.g. "src/**/*.rs" → prefix="src/", suffix="*.rs"
    if let Some(star_pos) = pattern.find("**") {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 2..];
        let suffix = suffix
            .strip_prefix('/')
            .or_else(|| suffix.strip_prefix('\\'))
            .unwrap_or(suffix);
        let base_dir = if prefix.is_empty() {
            ".".to_string()
        } else {
            prefix
                .trim_end_matches('/')
                .trim_end_matches('\\')
                .to_string()
        };
        let base_path = std::path::Path::new(&base_dir);
        if base_path.is_dir() {
            walk_dir_recursive(base_path, suffix, results);
        }
    } else {
        glob_simple(pattern, results)?;
    }
    Ok(())
}

fn walk_dir_recursive(dir: &std::path::Path, file_pattern: &str, results: &mut Vec<PyObjectRef>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Match directory itself if pattern is empty
                if file_pattern.is_empty() {
                    results.push(PyObject::str_val(CompactString::from(
                        path.to_string_lossy().to_string(),
                    )));
                }
                walk_dir_recursive(&path, file_pattern, results);
            } else if !file_pattern.is_empty() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if glob_match(file_pattern, &name) {
                    results.push(PyObject::str_val(CompactString::from(
                        path.to_string_lossy().to_string(),
                    )));
                }
            }
        }
    }
}

pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return pattern == text;
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_at(&pat, 0, &txt, 0)
}

fn glob_match_at(pat: &[char], mut pi: usize, txt: &[char], mut ti: usize) -> bool {
    while pi < pat.len() {
        match pat[pi] {
            '*' => {
                pi += 1;
                // Match zero or more characters
                for k in ti..=txt.len() {
                    if glob_match_at(pat, pi, txt, k) {
                        return true;
                    }
                }
                return false;
            }
            '?' => {
                if ti >= txt.len() {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            '[' => {
                if ti >= txt.len() {
                    return false;
                }
                let c = txt[ti];
                pi += 1;
                let negate = pi < pat.len() && (pat[pi] == '!' || pat[pi] == '^');
                if negate {
                    pi += 1;
                }
                let mut matched = false;
                while pi < pat.len() && pat[pi] != ']' {
                    if pi + 2 < pat.len() && pat[pi + 1] == '-' {
                        if c >= pat[pi] && c <= pat[pi + 2] {
                            matched = true;
                        }
                        pi += 3;
                    } else {
                        if c == pat[pi] {
                            matched = true;
                        }
                        pi += 1;
                    }
                }
                if pi < pat.len() {
                    pi += 1;
                } // skip ']'
                if matched == negate {
                    return false;
                }
                ti += 1;
            }
            c => {
                if ti >= txt.len() || txt[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == txt.len()
}

// ── tempfile module (basic) ──

use std::sync::atomic::{AtomicU64, Ordering};

static TMPFILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Shared write buffers for NamedTemporaryFile instances, keyed by path.
#[allow(dead_code)]
static TMPFILE_BUFFERS: std::sync::LazyLock<Mutex<IndexMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

fn named_temporary_file(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract keyword args (mode, suffix, prefix, delete)
    let mut mode = String::from("w+b");
    let mut suffix = String::from("");
    let mut delete = true;
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
                    "prefix" => { /* ignored for now */ }
                    "delete" => delete = v.is_truthy(),
                    _ => {}
                }
            }
        }
    }

    let n = TMPFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "ferrython_ntf_{}{}{}",
        std::process::id(),
        n,
        suffix
    ));
    let path_str = path.to_string_lossy().to_string();
    let is_binary = mode.contains('b');

    // Open with read+write so both directions work
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| PyException::runtime_error(format!("tempfile: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::io::IntoRawFd;

        let fd = file.into_raw_fd();
        let state = Rc::new(PyCell::new((fd, false))); // (fd, closed)
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(&path_str)),
        );
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from(&mode)),
        );
        attrs.insert(CompactString::from("_delete"), PyObject::bool_val(delete));
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // write(data)
        let s1 = state.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("write", move |a| {
                let g = s1.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let data_arg = if a.len() > 1 {
                    &a[1]
                } else if !a.is_empty() {
                    &a[0]
                } else {
                    return Err(PyException::type_error("write requires data"));
                };
                let data_bytes = match &data_arg.payload {
                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => data_arg.py_to_string().into_bytes(),
                };
                let n = unsafe {
                    libc::write(
                        fd,
                        data_bytes.as_ptr() as *const libc::c_void,
                        data_bytes.len(),
                    )
                };
                if n < 0 {
                    return Err(PyException::os_error("write failed".to_string()));
                }
                Ok(PyObject::int(n as i64))
            }),
        );

        // read([size])
        let s2 = state.clone();
        let is_bin_r = is_binary;
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("read", move |a| {
                let g = s2.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let size: isize = if a.len() > 1 {
                    a[1].as_int().unwrap_or(-1) as isize
                } else if !a.is_empty() {
                    a[0].as_int().unwrap_or(-1) as isize
                } else {
                    -1
                };
                let buf = if size < 0 {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    loop {
                        let n = unsafe {
                            libc::read(fd, tmp.as_mut_ptr() as *mut libc::c_void, tmp.len())
                        };
                        if n <= 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n as usize]);
                    }
                    buf
                } else {
                    let mut buf = vec![0u8; size as usize];
                    let n =
                        unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                    if n < 0 {
                        return Err(PyException::os_error("read failed".to_string()));
                    }
                    buf.truncate(n as usize);
                    buf
                };
                if is_bin_r {
                    Ok(PyObject::bytes(buf))
                } else {
                    Ok(PyObject::str_val(CompactString::from(
                        String::from_utf8_lossy(&buf).as_ref(),
                    )))
                }
            }),
        );

        // seek(offset, whence=0)
        let s3 = state.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("seek", move |a| {
                let g = s3.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let offset = if a.len() > 1 {
                    a[1].as_int().unwrap_or(0) as i64
                } else if !a.is_empty() {
                    a[0].as_int().unwrap_or(0) as i64
                } else {
                    0i64
                };
                let whence = if a.len() > 2 {
                    a[2].as_int().unwrap_or(0) as i32
                } else {
                    0i32
                };
                let pos = unsafe { libc::lseek(fd, offset as libc::off_t, whence) };
                if pos < 0 {
                    return Err(PyException::os_error("seek failed".to_string()));
                }
                Ok(PyObject::int(pos as i64))
            }),
        );

        // tell()
        let s4 = state.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("tell", move |_a| {
                let g = s4.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let pos = unsafe { libc::lseek(g.0, 0, libc::SEEK_CUR) };
                Ok(PyObject::int(pos as i64))
            }),
        );

        // flush()
        let s5 = state.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("flush", move |_a| {
                let g = s5.read();
                if !g.1 {
                    unsafe {
                        libc::fsync(g.0);
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // close()
        let s6 = state.clone();
        let ps_c = path_str.clone();
        let del_c = delete;
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("close", move |_| {
                let mut g = s6.write();
                if !g.1 {
                    g.1 = true;
                    unsafe {
                        libc::close(g.0);
                    }
                    if del_c {
                        std::fs::remove_file(&ps_c).ok();
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__(self)
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_function("__enter__", |args| {
                if !args.is_empty() {
                    Ok(args[0].clone())
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        // __exit__ — close + optionally delete
        let s7 = state.clone();
        let ps_e = path_str.clone();
        let del_e = delete;
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_| {
                let mut g = s7.write();
                if !g.1 {
                    g.1 = true;
                    unsafe {
                        libc::close(g.0);
                    }
                    if del_e {
                        std::fs::remove_file(&ps_e).ok();
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );

        attrs.insert(
            CompactString::from("_bind_methods"),
            PyObject::bool_val(true),
        );

        let class = PyObject::class(
            CompactString::from("_io.BufferedRandom"),
            vec![],
            IndexMap::new(),
        );
        Ok(PyObject::instance_with_attrs(class, attrs))
    }
    #[cfg(not(unix))]
    {
        let _ = (path_str, is_binary, delete);
        Err(PyException::not_implemented_error(
            "NamedTemporaryFile not available on this platform",
        ))
    }
}

pub fn create_tempfile_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_name(prefix: &str, suffix: &str) -> String {
        // Use counter + process ID + random bits to generate unique names
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let rand_bits: u64 = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            n.hash(&mut h);
            pid.hash(&mut h);
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut h);
            h.finish()
        };
        format!(
            "{}{}{}{}{}{}",
            std::env::temp_dir().to_string_lossy(),
            std::path::MAIN_SEPARATOR,
            prefix,
            rand_bits,
            n,
            suffix
        )
    }

    make_module(
        "tempfile",
        vec![
            (
                "gettempdir",
                make_builtin(|_| {
                    Ok(PyObject::str_val(CompactString::from(
                        std::env::temp_dir().to_string_lossy().to_string(),
                    )))
                }),
            ),
            (
                "mkdtemp",
                make_builtin(|args| {
                    let mut suffix = String::new();
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("suffix")))
                            {
                                suffix = v.py_to_string();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    let dir = temp_name(&prefix, &suffix);
                    std::fs::create_dir(&dir)
                        .map_err(|e| PyException::runtime_error(format!("mkdtemp: {}", e)))?;
                    Ok(PyObject::str_val(CompactString::from(dir)))
                }),
            ),
            (
                "mkstemp",
                make_builtin(|args| {
                    let mut suffix = String::new();
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("suffix")))
                            {
                                suffix = v.py_to_string();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    let path = temp_name(&prefix, &suffix);
                    // Open with read+write (O_RDWR | O_CREAT | O_EXCL) like CPython
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|e| PyException::runtime_error(format!("mkstemp: {}", e)))?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::IntoRawFd;
                        let fd = file.into_raw_fd();
                        Ok(PyObject::tuple(vec![
                            PyObject::int(fd as i64),
                            PyObject::str_val(CompactString::from(path)),
                        ]))
                    }
                    #[cfg(not(unix))]
                    {
                        drop(file);
                        Ok(PyObject::tuple(vec![
                            PyObject::int(0),
                            PyObject::str_val(CompactString::from(path)),
                        ]))
                    }
                }),
            ),
            (
                "mktemp",
                make_builtin(|args| {
                    let mut suffix = String::new();
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("suffix")))
                            {
                                suffix = v.py_to_string();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(temp_name(
                        &prefix, &suffix,
                    ))))
                }),
            ),
            ("NamedTemporaryFile", make_builtin(named_temporary_file)),
            ("TemporaryFile", make_builtin(named_temporary_file)),
            ("SpooledTemporaryFile", make_builtin(named_temporary_file)),
            (
                "_TemporaryFileWrapper",
                PyObject::class(CompactString::from("_TemporaryFileWrapper"), vec![], {
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__init__"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                    ns
                }),
            ),
            (
                "TemporaryDirectory",
                make_builtin(|args| {
                    let mut prefix = "tmp".to_string();
                    for arg in args {
                        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("prefix")))
                            {
                                prefix = v.py_to_string();
                            }
                        }
                    }
                    let dir = temp_name(&prefix, "");
                    std::fs::create_dir_all(&dir).map_err(|e| {
                        PyException::runtime_error(format!("TemporaryDirectory: {}", e))
                    })?;

                    let cls = PyObject::class(
                        CompactString::from("TemporaryDirectory"),
                        vec![],
                        IndexMap::new(),
                    );
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("name"),
                        PyObject::str_val(CompactString::from(&dir)),
                    );

                    let dir_enter = dir.clone();
                    attrs.insert(
                        CompactString::from("__enter__"),
                        PyObject::native_closure("TemporaryDirectory.__enter__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(dir_enter.as_str())))
                        }),
                    );
                    let dir_exit = dir.clone();
                    attrs.insert(
                        CompactString::from("__exit__"),
                        PyObject::native_closure("TemporaryDirectory.__exit__", move |_| {
                            let _ = std::fs::remove_dir_all(&dir_exit);
                            Ok(PyObject::bool_val(false))
                        }),
                    );
                    let dir_cleanup = dir;
                    attrs.insert(
                        CompactString::from("cleanup"),
                        PyObject::native_closure("TemporaryDirectory.cleanup", move |_| {
                            let _ = std::fs::remove_dir_all(&dir_cleanup);
                            Ok(PyObject::none())
                        }),
                    );
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }),
            ),
        ],
    )
}

// ── fnmatch module ──

pub fn create_io_module() -> PyObjectRef {
    make_module(
        "io",
        vec![
            ("StringIO", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(io_string_io_init),
                );
                PyObject::class(CompactString::from("StringIO"), vec![], ns)
            }),
            ("BytesIO", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(io_bytes_io_init),
                );
                PyObject::class(CompactString::from("BytesIO"), vec![], ns)
            }),
            ("TextIOWrapper", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(io_text_io_wrapper_init),
                );
                PyObject::class(CompactString::from("TextIOWrapper"), vec![], ns)
            }),
            ("BufferedReader", make_builtin(io_buffered_reader)),
            ("BufferedWriter", make_builtin(io_buffered_writer)),
            (
                "IOBase",
                PyObject::class(CompactString::from("IOBase"), vec![], IndexMap::new()),
            ),
            ("RawIOBase", {
                let mut ns = IndexMap::new();
                // Marker methods — actual logic is handled by VM-level intercept
                ns.insert(
                    CompactString::from("read"),
                    PyObject::native_function("RawIOBase.read", |_| {
                        Err(PyException::runtime_error(
                            "RawIOBase.read requires VM intercept",
                        ))
                    }),
                );
                ns.insert(
                    CompactString::from("readall"),
                    PyObject::native_function("RawIOBase.readall", |_| {
                        Err(PyException::runtime_error(
                            "RawIOBase.readall requires VM intercept",
                        ))
                    }),
                );
                PyObject::class(CompactString::from("RawIOBase"), vec![], ns)
            }),
            (
                "BufferedIOBase",
                PyObject::class(
                    CompactString::from("BufferedIOBase"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            ("BufferedRandom", make_builtin(io_buffered_reader)), // BufferedRandom ≈ BufferedReader for now
            (
                "BufferedRWPair",
                PyObject::class(
                    CompactString::from("BufferedRWPair"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "FileIO",
                PyObject::class(CompactString::from("FileIO"), vec![], IndexMap::new()),
            ),
            (
                "TextIOBase",
                PyObject::class(CompactString::from("TextIOBase"), vec![], IndexMap::new()),
            ),
            (
                "UnsupportedOperation",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            ("SEEK_SET", PyObject::int(0)),
            ("SEEK_CUR", PyObject::int(1)),
            ("SEEK_END", PyObject::int(2)),
            ("DEFAULT_BUFFER_SIZE", PyObject::int(8192)),
            // io.text_encoding(encoding, stacklevel=2) — Python 3.11+
            (
                "text_encoding",
                make_builtin(|args: &[PyObjectRef]| {
                    // If encoding is None or not provided, return "locale" (CPython default)
                    if args.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("locale")));
                    }
                    if matches!(&args[0].payload, PyObjectPayload::None) {
                        return Ok(PyObject::str_val(CompactString::from("locale")));
                    }
                    Ok(args[0].clone())
                }),
            ),
            (
                "open",
                make_builtin(|args| {
                    // io.open — replicates builtins.open() behavior
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "open() requires at least 1 argument",
                        ));
                    }
                    let path = args[0].py_to_string();
                    let mode = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "r".to_string()
                    };
                    let is_binary = mode.contains('b');
                    let is_write = mode.contains('w') || mode.contains('a') || mode.contains('x');

                    let content = if is_write {
                        if mode.contains('a') {
                            std::fs::read_to_string(&path).unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        std::fs::read_to_string(&path)
                            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?
                    };

                    let data: Rc<PyCell<(String, usize, bool)>> =
                        Rc::new(PyCell::new((content, 0, false)));
                    let cls =
                        PyObject::class(CompactString::from("_io_file"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut a = d.attrs.write();
                        a.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from(path.as_str())),
                        );
                        a.insert(
                            CompactString::from("mode"),
                            PyObject::str_val(CompactString::from(mode.as_str())),
                        );
                        a.insert(CompactString::from("closed"), PyObject::bool_val(false));
                        let d1 = data.clone();
                        a.insert(
                            CompactString::from("read"),
                            PyObject::native_closure("read", move |rargs| {
                                let g = d1.read();
                                let remaining = &g.0[g.1..];
                                let n = rargs.first().and_then(|a| a.as_int());
                                let text = match n {
                                    Some(n) if n >= 0 => {
                                        let end = (g.1 + n as usize).min(g.0.len());
                                        g.0[g.1..end].to_string()
                                    }
                                    _ => remaining.to_string(),
                                };
                                drop(g);
                                let len = text.len();
                                d1.write().1 += len;
                                if is_binary {
                                    Ok(PyObject::bytes(text.into_bytes()))
                                } else {
                                    Ok(PyObject::str_val(CompactString::from(text)))
                                }
                            }),
                        );
                        let d2 = data.clone();
                        a.insert(
                            CompactString::from("readline"),
                            PyObject::native_closure("readline", move |_| {
                                let g = d2.read();
                                let remaining = &g.0[g.1..];
                                if remaining.is_empty() {
                                    return Ok(PyObject::str_val(CompactString::from("")));
                                }
                                let line = if let Some(idx) = remaining.find('\n') {
                                    &remaining[..=idx]
                                } else {
                                    remaining
                                };
                                let r = line.to_string();
                                drop(g);
                                d2.write().1 += r.len();
                                Ok(PyObject::str_val(CompactString::from(r)))
                            }),
                        );
                        let d3 = data.clone();
                        let p2 = path.clone();
                        let m2 = mode.clone();
                        a.insert(
                            CompactString::from("write"),
                            PyObject::native_closure("write", move |wargs| {
                                if wargs.is_empty() {
                                    return Err(PyException::type_error("write requires data"));
                                }
                                let text = wargs[wargs.len() - 1].py_to_string();
                                let len = text.len();
                                d3.write().0.push_str(&text);
                                // Write to disk
                                let g = d3.read();
                                if m2.contains('a') {
                                    use std::io::Write;
                                    let mut f = std::fs::OpenOptions::new()
                                        .append(true)
                                        .create(true)
                                        .open(&p2)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                    f.write_all(text.as_bytes())
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                } else {
                                    std::fs::write(&p2, &g.0)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                }
                                drop(g);
                                Ok(PyObject::int(len as i64))
                            }),
                        );
                        let d4 = data.clone();
                        let inst_for_close = inst.clone();
                        a.insert(
                            CompactString::from("close"),
                            PyObject::native_closure("close", move |_| {
                                d4.write().2 = true;
                                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                                    d.attrs.write().insert(
                                        CompactString::from("closed"),
                                        PyObject::bool_val(true),
                                    );
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        a.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("__enter__", {
                                let inst2 = inst.clone();
                                move |_| Ok(inst2.clone())
                            }),
                        );
                        let inst_for_exit = inst.clone();
                        let d5 = data.clone();
                        a.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("__exit__", move |_| {
                                d5.write().2 = true;
                                if let PyObjectPayload::Instance(ref d) = inst_for_exit.payload {
                                    d.attrs.write().insert(
                                        CompactString::from("closed"),
                                        PyObject::bool_val(true),
                                    );
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        let d6 = data.clone();
                        a.insert(
                            CompactString::from("seek"),
                            PyObject::native_closure("seek", move |sargs| {
                                let pos =
                                    sargs.first().and_then(|a| a.as_int()).unwrap_or(0) as usize;
                                d6.write().1 = pos;
                                Ok(PyObject::int(pos as i64))
                            }),
                        );
                        let d7 = data.clone();
                        a.insert(
                            CompactString::from("tell"),
                            PyObject::native_closure("tell", move |_| {
                                Ok(PyObject::int(d7.read().1 as i64))
                            }),
                        );
                        a.insert(
                            CompactString::from("flush"),
                            make_builtin(|_| Ok(PyObject::none())),
                        );
                        a.insert(
                            CompactString::from("readable"),
                            PyObject::native_closure("readable", {
                                let m = mode.clone();
                                move |_| Ok(PyObject::bool_val(m.contains('r')))
                            }),
                        );
                        a.insert(
                            CompactString::from("writable"),
                            PyObject::native_closure("writable", {
                                let m = mode.clone();
                                move |_| Ok(PyObject::bool_val(is_write || m.contains('+')))
                            }),
                        );
                        a.insert(
                            CompactString::from("seekable"),
                            make_builtin(|_| Ok(PyObject::bool_val(true))),
                        );
                        a.insert(
                            CompactString::from("isatty"),
                            make_builtin(|_| Ok(PyObject::bool_val(false))),
                        );
                        // fileno() — open a real OS fd for the path so mmap etc. can work
                        let fpath = path.clone();
                        let fmode = mode.clone();
                        a.insert(
                            CompactString::from("fileno"),
                            PyObject::native_closure("fileno", move |_: &[PyObjectRef]| {
                                use std::os::unix::io::IntoRawFd;
                                let f = if fmode.contains('w') || fmode.contains('a') {
                                    std::fs::OpenOptions::new()
                                        .read(true)
                                        .write(true)
                                        .open(&fpath)
                                } else {
                                    std::fs::File::open(&fpath)
                                };
                                match f {
                                    Ok(file) => Ok(PyObject::int(file.into_raw_fd() as i64)),
                                    Err(e) => {
                                        Err(PyException::os_error(format!("{}: '{}'", e, fpath)))
                                    }
                                }
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "FileIO",
                make_builtin(|args| {
                    // FileIO(name, mode='r') -- thin wrapper around OS file descriptor
                    if args.is_empty() {
                        return Err(PyException::type_error("FileIO requires a file path or fd"));
                    }
                    let name = args[0].py_to_string();
                    let mode = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "r".to_string()
                    };
                    let file = if mode.contains('w') {
                        std::fs::File::create(&name)
                            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, name)))?
                    } else {
                        std::fs::File::open(&name)
                            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, name)))?
                    };
                    let buf: Rc<PyCell<Option<std::fs::File>>> = Rc::new(PyCell::new(Some(file)));
                    let cls =
                        PyObject::class(CompactString::from("FileIO"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(d) = &inst.payload {
                        let mut a = d.attrs.write();
                        a.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from(name)),
                        );
                        a.insert(
                            CompactString::from("mode"),
                            PyObject::str_val(CompactString::from(mode.as_str())),
                        );
                        a.insert(CompactString::from("closed"), PyObject::bool_val(false));
                        let buf2 = buf.clone();
                        a.insert(
                            CompactString::from("read"),
                            PyObject::native_closure("FileIO.read", move |_| {
                                use std::io::Read;
                                let mut guard = buf2.write();
                                if let Some(ref mut f) = *guard {
                                    let mut data = Vec::new();
                                    f.read_to_end(&mut data)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                    Ok(PyObject::bytes(data))
                                } else {
                                    Err(PyException::value_error("I/O operation on closed file"))
                                }
                            }),
                        );
                        let buf3 = buf.clone();
                        a.insert(
                            CompactString::from("write"),
                            PyObject::native_closure("FileIO.write", move |wargs| {
                                use std::io::Write;
                                if wargs.is_empty() {
                                    return Err(PyException::type_error("write requires data"));
                                }
                                let mut guard = buf3.write();
                                if let Some(ref mut f) = *guard {
                                    let data = match &wargs[0].payload {
                                        PyObjectPayload::Bytes(b) => (**b).clone(),
                                        _ => wargs[0].py_to_string().into_bytes(),
                                    };
                                    let n = f
                                        .write(&data)
                                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                                    Ok(PyObject::int(n as i64))
                                } else {
                                    Err(PyException::value_error("I/O operation on closed file"))
                                }
                            }),
                        );
                        let buf4 = buf.clone();
                        a.insert(
                            CompactString::from("close"),
                            PyObject::native_closure("FileIO.close", move |_| {
                                *buf4.write() = None;
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
        ],
    )
}

/// StringIO.__init__: installs string buffer methods on self.
/// Called as __init__(self, initial_value="")
fn io_string_io_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = optional initial value
    if args.is_empty() {
        return Err(PyException::type_error("StringIO.__init__() requires self"));
    }
    let self_obj = args[0].clone();
    let initial = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        String::new()
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__stringio__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));

        let buf: Rc<PyCell<String>> = Rc::new(PyCell::new(initial));
        let pos: Rc<PyCell<usize>> = Rc::new(PyCell::new(0));

        // write(s) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("StringIO.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() takes 1 argument"));
                }
                let s = a[0].py_to_string();
                let len = s.len();
                let mut bw = b.write();
                let mut pw = p.write();
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
            }),
        );

        // read(size=-1) → str
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("StringIO.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let end = if size < 0 {
                    br.len()
                } else {
                    (cur + size as usize).min(br.len())
                };
                let result = &br[cur..end];
                *pw = end;
                Ok(PyObject::str_val(CompactString::from(result)))
            }),
        );

        // getvalue() → str
        let b = buf.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("StringIO.getvalue", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(b.read().as_str())))
            }),
        );

        // seek(offset, whence=0) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("StringIO.seek", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("seek() takes at least 1 argument"));
                }
                let offset = a[0].as_int().unwrap_or(0);
                let whence = if a.len() > 1 {
                    a[1].as_int().unwrap_or(0)
                } else {
                    0
                };
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
            }),
        );

        // tell() → int
        let p = pos.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("StringIO.tell", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(*p.read() as i64))
            }),
        );

        // truncate(size=None) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("truncate"),
            PyObject::native_closure("StringIO.truncate", move |a: &[PyObjectRef]| {
                let mut bw = b.write();
                let size = if a.is_empty() || matches!(&a[0].payload, PyObjectPayload::None) {
                    *p.read()
                } else {
                    a[0].as_int().unwrap_or(0) as usize
                };
                bw.truncate(size);
                Ok(PyObject::int(size as i64))
            }),
        );

        // readline() → str
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("StringIO.readline", move |_: &[PyObjectRef]| {
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let rest = &br[cur..];
                let end = rest.find('\n').map(|i| cur + i + 1).unwrap_or(br.len());
                *pw = end;
                Ok(PyObject::str_val(CompactString::from(&br[cur..end])))
            }),
        );

        // readlines() → list[str]
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("StringIO.readlines", move |_: &[PyObjectRef]| {
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::list(vec![]));
                }
                let rest = &br[cur..];
                let lines: Vec<PyObjectRef> = rest
                    .split_inclusive('\n')
                    .map(|line| PyObject::str_val(CompactString::from(line)))
                    .collect();
                *pw = br.len();
                Ok(PyObject::list(lines))
            }),
        );

        // close()
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("StringIO.close", move |_| {
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );
        // flush()
        attrs.insert(
            CompactString::from("flush"),
            make_builtin(|_| Ok(PyObject::none())),
        );

        // Protocol methods
        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("seekable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("isatty"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        attrs.insert(
            CompactString::from("fileno"),
            make_builtin(|_| {
                Err(PyException::runtime_error(
                    "StringIO does not use a file descriptor",
                ))
            }),
        );

        // closed property
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // __enter__ / __exit__ for context manager
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("StringIO.__enter__", move |_: &[PyObjectRef]| {
                Ok(inst_ref.clone())
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );

        // __iter__ — iterates lines
        let rl_buf = buf.clone();
        let rl_pos = pos.clone();
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("StringIO.__iter__", move |_: &[PyObjectRef]| {
                let b = rl_buf.read();
                let p = *rl_pos.read();
                let remaining = if p < b.len() { &b[p..] } else { "" };
                let mut lines: Vec<PyObjectRef> = Vec::new();
                for line in remaining.split('\n') {
                    if !line.is_empty() || lines.is_empty() {
                        lines.push(PyObject::str_val(CompactString::from(format!(
                            "{}\n",
                            line
                        ))));
                    }
                }
                // Fix last line if original didn't end with \n
                if !remaining.ends_with('\n') && !lines.is_empty() {
                    let last_idx = lines.len() - 1;
                    let last = lines[last_idx].py_to_string();
                    lines[last_idx] =
                        PyObject::str_val(CompactString::from(last.trim_end_matches('\n')));
                }
                Ok(PyObject::list(lines))
            }),
        );
    }
    Ok(PyObject::none())
}

/// Build a BytesIO instance with methods attached.
/// BytesIO.__init__: installs buffer methods on self.
/// Called as __init__(self, initial_bytes=b"")
fn io_bytes_io_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = optional initial bytes
    if args.is_empty() {
        return Err(PyException::type_error("BytesIO.__init__() requires self"));
    }
    let self_obj = args[0].clone();
    let initial = if args.len() > 1 {
        if let PyObjectPayload::Bytes(b) = &args[1].payload {
            (**b).clone()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__bytesio__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));

        let buf: Rc<PyCell<Vec<u8>>> = Rc::new(PyCell::new(initial));
        let pos: Rc<PyCell<usize>> = Rc::new(PyCell::new(0));
        let closed_flag: Rc<PyCell<bool>> = Rc::new(PyCell::new(false));

        // write(b) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("BytesIO.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() takes 1 argument"));
                }
                let data = match &a[0].payload {
                    PyObjectPayload::Bytes(v) => (**v).clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => return Err(PyException::type_error("a bytes-like object is required")),
                };
                let len = data.len();
                let mut bw = b.write();
                let mut pw = p.write();
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
            }),
        );

        // read(size=-1) → bytes
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("BytesIO.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let br = b.read();
                let mut pw = p.write();
                let cur = *pw;
                if cur >= br.len() {
                    return Ok(PyObject::bytes(vec![]));
                }
                let end = if size < 0 {
                    br.len()
                } else {
                    (cur + size as usize).min(br.len())
                };
                let result = br[cur..end].to_vec();
                *pw = end;
                Ok(PyObject::bytes(result))
            }),
        );

        // getvalue() → bytes
        let b = buf.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("BytesIO.getvalue", move |_: &[PyObjectRef]| {
                Ok(PyObject::bytes(b.read().clone()))
            }),
        );

        // seek(offset, whence=0) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BytesIO.seek", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("seek() takes at least 1 argument"));
                }
                let offset = a[0].as_int().unwrap_or(0);
                let whence = if a.len() > 1 {
                    a[1].as_int().unwrap_or(0)
                } else {
                    0
                };
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
            }),
        );

        // tell() → int
        let p = pos.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BytesIO.tell", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(*p.read() as i64))
            }),
        );

        // truncate(size=None) → int
        let b = buf.clone();
        let p = pos.clone();
        attrs.insert(
            CompactString::from("truncate"),
            PyObject::native_closure("BytesIO.truncate", move |a: &[PyObjectRef]| {
                let mut bw = b.write();
                let size = if a.is_empty() || matches!(&a[0].payload, PyObjectPayload::None) {
                    *p.read()
                } else {
                    a[0].as_int().unwrap_or(0) as usize
                };
                bw.truncate(size);
                Ok(PyObject::int(size as i64))
            }),
        );

        // close()
        let cf = closed_flag.clone();
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BytesIO.close", move |_args: &[PyObjectRef]| {
                *cf.write() = true;
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                    d.attrs
                        .write()
                        .insert(CompactString::from("_closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );
        // flush()
        attrs.insert(
            CompactString::from("flush"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        // Protocol methods
        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("seekable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("isatty"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );

        // readline()
        let rl_buf = buf.clone();
        let rl_pos = pos.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("BytesIO.readline", move |_: &[PyObjectRef]| {
                let b = rl_buf.read();
                let mut p = rl_pos.write();
                let start = *p;
                if start >= b.len() {
                    return Ok(PyObject::bytes(vec![]));
                }
                let end = b[start..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map(|i| start + i + 1)
                    .unwrap_or(b.len());
                *p = end;
                Ok(PyObject::bytes(b[start..end].to_vec()))
            }),
        );

        // readlines() — read all remaining lines
        let rls_buf = buf.clone();
        let rls_pos = pos.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("BytesIO.readlines", move |_: &[PyObjectRef]| {
                let b = rls_buf.read();
                let mut p = rls_pos.write();
                let mut lines = Vec::new();
                while *p < b.len() {
                    let start = *p;
                    let end = b[start..]
                        .iter()
                        .position(|&c| c == b'\n')
                        .map(|i| start + i + 1)
                        .unwrap_or(b.len());
                    *p = end;
                    lines.push(PyObject::bytes(b[start..end].to_vec()));
                }
                Ok(PyObject::list(lines))
            }),
        );

        // writelines(lines) — write a list of bytes objects
        let wl_buf = buf.clone();
        let wl_pos = pos.clone();
        attrs.insert(
            CompactString::from("writelines"),
            PyObject::native_closure("BytesIO.writelines", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Ok(PyObject::none());
                }
                let items = a[0].to_list()?;
                let mut b = wl_buf.write();
                let mut p = wl_pos.write();
                for item in items {
                    if let PyObjectPayload::Bytes(data) = &item.payload {
                        let d = data;
                        let pos_val = *p;
                        if pos_val == b.len() {
                            b.extend_from_slice(d);
                        } else {
                            let end = (pos_val + d.len()).min(b.len());
                            b.splice(pos_val..end, d.iter().cloned());
                        }
                        *p += d.len();
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__ / __exit__
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BytesIO.__enter__", move |_: &[PyObjectRef]| {
                Ok(inst_ref.clone())
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(PyObject::none())
}

/// TextIOWrapper.__init__: installs buffer-delegating methods on self.
/// Called as __init__(self, buffer, encoding='utf-8', errors='strict', ...)
fn io_text_io_wrapper_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = buffer, optional encoding/kwargs
    if args.len() < 2 {
        return Err(PyException::type_error(
            "TextIOWrapper.__init__() requires a buffer argument",
        ));
    }
    let self_obj = args[0].clone();
    let buffer = args[1].clone();
    let encoding = if args.len() > 2 {
        args[2].py_to_string()
    } else {
        "utf-8".to_string()
    };
    // Extract kwargs if trailing dict
    let (enc, _errors) = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            let e = r
                .get(&HashableKey::str_key(CompactString::from("encoding")))
                .map(|v| v.py_to_string())
                .unwrap_or(encoding);
            let er = r
                .get(&HashableKey::str_key(CompactString::from("errors")))
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| "strict".to_string());
            (e, er)
        } else {
            (encoding, "strict".to_string())
        }
    } else {
        (encoding, "strict".to_string())
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("buffer"), buffer.clone());
        attrs.insert(
            CompactString::from("encoding"),
            PyObject::str_val(CompactString::from(&enc)),
        );
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from("r")),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from("<TextIOWrapper>")),
        );

        // read(size=-1) — decode bytes from buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("TextIOWrapper.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                if let Some(read_fn) = buf.get_attr("read") {
                    let bytes_result = if size < 0 {
                        call_native(&read_fn, &[])?
                    } else {
                        call_native(&read_fn, &[PyObject::int(size)])?
                    };
                    if let PyObjectPayload::Bytes(b) = &bytes_result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(bytes_result)
                    }
                } else {
                    Err(PyException::type_error("buffer has no read method"))
                }
            }),
        );

        // write(s) — encode str to bytes and write to buffer (rejects bytes like CPython)
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("TextIOWrapper.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() requires 1 argument"));
                }
                // TextIOWrapper only accepts str, not bytes
                if matches!(&a[0].payload, PyObjectPayload::Bytes(_)) {
                    return Err(PyException::type_error(
                        "write() argument must be str, not bytes",
                    ));
                }
                let text = a[0].py_to_string();
                let bytes_obj = PyObject::bytes(text.as_bytes().to_vec());
                if let Some(write_fn) = buf.get_attr("write") {
                    call_native(&write_fn, &[bytes_obj])
                } else {
                    Err(PyException::type_error("buffer has no write method"))
                }
            }),
        );

        // readline() — read line from buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("TextIOWrapper.readline", move |_: &[PyObjectRef]| {
                if let Some(readline_fn) = buf.get_attr("readline") {
                    let result = call_native(&readline_fn, &[])?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(PyException::type_error("buffer has no readline method"))
                }
            }),
        );

        // readlines(hint=-1) — read all lines
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("TextIOWrapper.readlines", move |a: &[PyObjectRef]| {
                let hint = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let mut lines = Vec::new();
                let mut total_bytes = 0i64;
                loop {
                    if let Some(readline_fn) = buf.get_attr("readline") {
                        let result = call_native(&readline_fn, &[])?;
                        let line_str = if let PyObjectPayload::Bytes(b) = &result.payload {
                            String::from_utf8_lossy(b).to_string()
                        } else {
                            result.py_to_string()
                        };
                        if line_str.is_empty() {
                            break;
                        }
                        total_bytes += line_str.len() as i64;
                        lines.push(PyObject::str_val(CompactString::from(line_str)));
                        if hint > 0 && total_bytes >= hint {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                Ok(PyObject::list(lines))
            }),
        );

        // writelines(lines) — write an iterable of strings
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("writelines"),
            PyObject::native_closure("TextIOWrapper.writelines", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writelines() requires 1 argument"));
                }
                if let Some(write_fn) = buf.get_attr("write") {
                    if let PyObjectPayload::List(items) = &a[0].payload {
                        for item in items.read().iter() {
                            let text = item.py_to_string();
                            let bytes_obj = PyObject::bytes(text.as_bytes().to_vec());
                            call_native(&write_fn, &[bytes_obj])?;
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // seek/tell — delegate to buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("TextIOWrapper.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = buf.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("TextIOWrapper.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = buf.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        // flush — delegate to buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("TextIOWrapper.flush", move |_: &[PyObjectRef]| {
                if let Some(flush_fn) = buf.get_attr("flush") {
                    call_native(&flush_fn, &[])
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        // readable/writable/seekable
        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("seekable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );

        // close — delegate to buffer and mark closed
        let buf = buffer.clone();
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("TextIOWrapper.close", move |_| {
                if let Some(close_fn) = buf.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__ / __exit__
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("TextIOWrapper.__enter__", move |_| Ok(inst_ref.clone())),
        );
        let inst_for_exit = self_obj.clone();
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("TextIOWrapper.__exit__", move |_| {
                if let Some(close_fn) = buf.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_exit.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::bool_val(false))
            }),
        );

        // getvalue() — delegate to buffer (common for StringIO/BytesIO wrappers)
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("TextIOWrapper.getvalue", move |_: &[PyObjectRef]| {
                if let Some(gv) = buf.get_attr("getvalue") {
                    let result = call_native(&gv, &[])?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(PyException::attribute_error(
                        "underlying buffer has no getvalue",
                    ))
                }
            }),
        );
    }
    Ok(PyObject::none())
}

/// BufferedReader: wraps a raw binary stream with buffering
fn io_buffered_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "BufferedReader() requires a raw stream",
        ));
    }
    let raw = args[0].clone();
    let cls = PyObject::class(
        CompactString::from("BufferedReader"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("raw"), raw.clone());

        let r = raw.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("BufferedReader.read", move |a: &[PyObjectRef]| {
                if let Some(read_fn) = r.get_attr("read") {
                    call_native(&read_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no read method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("BufferedReader.readline", move |a: &[PyObjectRef]| {
                if let Some(readline_fn) = r.get_attr("readline") {
                    call_native(&readline_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no readline method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("BufferedReader.readlines", move |_: &[PyObjectRef]| {
                let mut lines = Vec::new();
                loop {
                    if let Some(readline_fn) = r.get_attr("readline") {
                        let result = call_native(&readline_fn, &[])?;
                        let is_empty = match &result.payload {
                            PyObjectPayload::Bytes(b) => b.is_empty(),
                            _ => result.py_to_string().is_empty(),
                        };
                        if is_empty {
                            break;
                        }
                        lines.push(result);
                    } else {
                        break;
                    }
                }
                Ok(PyObject::list(lines))
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BufferedReader.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = r.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BufferedReader.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = r.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        let inst_for_close = inst.clone();
        let r = raw.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BufferedReader.close", move |_| {
                if let Some(close_fn) = r.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        let inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BufferedReader.__enter__", move |_| Ok(inst_ref.clone())),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(inst)
}

/// BufferedWriter: wraps a raw binary stream with write buffering
fn io_buffered_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "BufferedWriter() requires a raw stream",
        ));
    }
    let raw = args[0].clone();
    let cls = PyObject::class(
        CompactString::from("BufferedWriter"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("raw"), raw.clone());

        let r = raw.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("BufferedWriter.write", move |a: &[PyObjectRef]| {
                if let Some(write_fn) = r.get_attr("write") {
                    call_native(&write_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no write method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("BufferedWriter.flush", move |_: &[PyObjectRef]| {
                if let Some(flush_fn) = r.get_attr("flush") {
                    call_native(&flush_fn, &[])
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BufferedWriter.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = r.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BufferedWriter.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = r.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        let inst_for_close = inst.clone();
        let r = raw;
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BufferedWriter.close", move |_| {
                if let Some(flush_fn) = r.get_attr("flush") {
                    let _ = call_native(&flush_fn, &[]);
                }
                if let Some(close_fn) = r.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        let inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BufferedWriter.__enter__", move |_| Ok(inst_ref.clone())),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(inst)
}

/// Helper: call a NativeFunction/NativeClosure directly
fn call_native(func: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Delegate to call_callable which handles native AND Python functions
    ferrython_core::object::call_callable(func, args)
}

mod subprocess;
mod zlib;

pub use subprocess::create_subprocess_module;
pub use zlib::create_zlib_module;
