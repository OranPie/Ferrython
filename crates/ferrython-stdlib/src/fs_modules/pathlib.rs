//! `pathlib` stdlib module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::sync::OnceLock;

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
