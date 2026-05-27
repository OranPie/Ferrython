use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── pathlib.Path methods ──

pub(super) fn simple_glob_match(pattern: &str, text: &str) -> bool {
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

pub(crate) fn call_pathlib_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let get_path = || -> String {
        inst.attrs
            .read()
            .get("_path")
            .map(|p| p.py_to_string())
            .unwrap_or_else(|| ".".to_string())
    };
    match method {
        "exists" => Ok(PyObject::bool_val(
            std::path::Path::new(&get_path()).exists(),
        )),
        "is_file" => Ok(PyObject::bool_val(
            std::path::Path::new(&get_path()).is_file(),
        )),
        "is_dir" => Ok(PyObject::bool_val(
            std::path::Path::new(&get_path()).is_dir(),
        )),
        "is_absolute" => Ok(PyObject::bool_val(
            std::path::Path::new(&get_path()).is_absolute(),
        )),
        "read_text" => {
            let path = get_path();
            let content = std::fs::read_to_string(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::str_val(CompactString::from(&content)))
        }
        "read_bytes" => {
            let path = get_path();
            let content = std::fs::read(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::bytes(content))
        }
        "write_text" => {
            check_args_min("write_text", args, 1)?;
            let path = get_path();
            let text = args[0].py_to_string();
            let len = text.len();
            std::fs::write(&path, &text)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::int(len as i64))
        }
        "write_bytes" => {
            check_args_min("write_bytes", args, 1)?;
            let path = get_path();
            let data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                _ => return Err(PyException::type_error("expected bytes")),
            };
            let len = data.len();
            std::fs::write(&path, &data)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::int(len as i64))
        }
        "mkdir" => {
            let path = get_path();
            // Check for parents=True, exist_ok=True kwargs
            let parents = args.iter().any(|a| {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    m.read()
                        .get(&HashableKey::str_key(CompactString::from("parents")))
                        .map(|v| v.is_truthy())
                        .unwrap_or(false)
                } else {
                    false
                }
            });
            let exist_ok = args.iter().any(|a| {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    m.read()
                        .get(&HashableKey::str_key(CompactString::from("exist_ok")))
                        .map(|v| v.is_truthy())
                        .unwrap_or(false)
                } else {
                    false
                }
            });
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
                Err(e) => Err(PyException::os_error(format!("{}: '{}'", e, path))),
            }
        }
        "rmdir" => {
            let path = get_path();
            std::fs::remove_dir(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }
        "unlink" => {
            let path = get_path();
            std::fs::remove_file(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }
        "iterdir" => {
            let path = get_path();
            let entries = std::fs::read_dir(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            let mut items = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path().to_string_lossy().to_string();
                items.push(PyObject::str_val(CompactString::from(&p)));
            }
            Ok(PyObject::list(items))
        }
        "glob" => {
            check_args_min("glob", args, 1)?;
            let base = get_path();
            let pattern = args[0].py_to_string();
            let dir = std::path::Path::new(&base);
            let mut results = Vec::new();
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if simple_glob_match(&pattern, &name) {
                        let full = entry.path().to_string_lossy().to_string();
                        results.push(PyObject::str_val(CompactString::from(&full)));
                    }
                }
            }
            Ok(PyObject::list(results))
        }
        "name" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(name)))
        }
        "stem" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let stem = p
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(stem)))
        }
        "suffix" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let ext = p
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(ext)))
        }
        "suffixes" => {
            let path = get_path();
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let parts: Vec<PyObjectRef> = name
                .match_indices('.')
                .map(|(i, _)| PyObject::str_val(CompactString::from(&name[i..])))
                .collect();
            // Actually need individual suffixes: ".tar.gz" → [".tar", ".gz"]
            let mut suffixes = Vec::new();
            let mut remaining = name.as_str();
            if let Some(first_dot) = remaining.find('.') {
                remaining = &remaining[first_dot..];
                for part in remaining.split('.').skip(1) {
                    suffixes.push(PyObject::str_val(CompactString::from(format!(".{}", part))));
                }
            }
            let _ = parts; // replaced
            Ok(PyObject::list(suffixes))
        }
        "parent" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let parent = p
                .parent()
                .map(|pp| pp.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            Ok(PyObject::str_val(CompactString::from(parent)))
        }
        "parents" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let mut parents = Vec::new();
            let mut cur = p.parent();
            while let Some(pp) = cur {
                parents.push(PyObject::str_val(CompactString::from(
                    pp.to_string_lossy().to_string(),
                )));
                cur = pp.parent();
                if pp.as_os_str().is_empty() {
                    break;
                }
            }
            Ok(PyObject::list(parents))
        }
        "parts" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let parts: Vec<PyObjectRef> = p
                .components()
                .map(|c| {
                    PyObject::str_val(CompactString::from(
                        c.as_os_str().to_string_lossy().to_string(),
                    ))
                })
                .collect();
            Ok(PyObject::tuple(parts))
        }
        "as_posix" => {
            let path = get_path();
            Ok(PyObject::str_val(CompactString::from(
                path.replace('\\', "/"),
            )))
        }
        "relative_to" => {
            check_args_min("relative_to", args, 1)?;
            let path = get_path();
            let base = args[0].py_to_string();
            if let Ok(rel) = std::path::Path::new(&path).strip_prefix(&base) {
                Ok(PyObject::str_val(CompactString::from(
                    rel.to_string_lossy().to_string(),
                )))
            } else {
                Err(PyException::value_error(format!(
                    "'{}' is not relative to '{}'",
                    path, base
                )))
            }
        }
        "is_symlink" => {
            let path = get_path();
            Ok(PyObject::bool_val(std::path::Path::new(&path).is_symlink()))
        }
        "absolute" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            if p.is_absolute() {
                Ok(PyObject::str_val(CompactString::from(path)))
            } else {
                let abs = std::env::current_dir().unwrap_or_default().join(p);
                Ok(PyObject::str_val(CompactString::from(
                    abs.to_string_lossy().to_string(),
                )))
            }
        }
        "resolve" => {
            let path = get_path();
            let resolved =
                std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::PathBuf::from(&path));
            Ok(PyObject::str_val(CompactString::from(
                resolved.to_string_lossy().to_string(),
            )))
        }
        "with_suffix" => {
            check_args_min("with_suffix", args, 1)?;
            let path = get_path();
            let new_suffix = args[0].py_to_string();
            let p = std::path::Path::new(&path);
            let new_path = p.with_extension(new_suffix.trim_start_matches('.'));
            Ok(PyObject::str_val(CompactString::from(
                new_path.to_string_lossy().to_string(),
            )))
        }
        "with_name" => {
            check_args_min("with_name", args, 1)?;
            let path = get_path();
            let new_name = args[0].py_to_string();
            let p = std::path::Path::new(&path);
            let new_path = p.with_file_name(&new_name);
            Ok(PyObject::str_val(CompactString::from(
                new_path.to_string_lossy().to_string(),
            )))
        }
        "joinpath" | "__truediv__" => {
            check_args_min("joinpath", args, 1)?;
            let base = get_path();
            let mut joined = std::path::PathBuf::from(&base);
            for arg in args {
                joined = joined.join(arg.py_to_string().as_str());
            }
            Ok(PyObject::str_val(CompactString::from(
                joined.to_string_lossy().to_string(),
            )))
        }
        "stat" => {
            let path = get_path();
            let meta = std::fs::metadata(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("st_size"),
                PyObject::int(meta.len() as i64),
            );
            ns.insert(CompactString::from("st_mode"), PyObject::int(0));
            let cls = PyObject::class(CompactString::from("stat_result"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(inst_data) = &inst_obj.payload {
                let mut attrs = inst_data.attrs.write();
                for (k, v) in ns {
                    attrs.insert(k, v);
                }
            }
            Ok(inst_obj)
        }
        "__str__" | "__repr__" | "__fspath__" => {
            Ok(PyObject::str_val(CompactString::from(get_path())))
        }
        "touch" => {
            let path = get_path();
            // touch(mode=0o666, exist_ok=True) — create file if doesn't exist
            let exist_ok = args.iter().any(|a| {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    m.read()
                        .get(&HashableKey::str_key(CompactString::from("exist_ok")))
                        .map(|v| v.is_truthy())
                        .unwrap_or(true)
                } else {
                    true
                }
            });
            let p = std::path::Path::new(&path);
            if p.exists() {
                if !exist_ok {
                    return Err(PyException::os_error(format!(
                        "FileExistsError: '{}'",
                        path
                    )));
                }
                // Update modification time by opening and closing
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            } else {
                std::fs::File::create(&path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            }
            Ok(PyObject::none())
        }
        "rglob" => {
            check_args_min("rglob", args, 1)?;
            let base = get_path();
            let pattern = args[0].py_to_string();
            let dir = std::path::Path::new(&base);
            let mut results = Vec::new();
            fn walk_dir_rglob(
                dir: &std::path::Path,
                pattern: &str,
                results: &mut Vec<PyObjectRef>,
            ) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy().to_string();
                        if simple_glob_match(pattern, &name) {
                            results.push(PyObject::str_val(CompactString::from(
                                path.to_string_lossy().to_string(),
                            )));
                        }
                        if path.is_dir() {
                            walk_dir_rglob(&path, pattern, results);
                        }
                    }
                }
            }
            walk_dir_rglob(dir, &pattern, &mut results);
            Ok(PyObject::list(results))
        }
        "chmod" => {
            check_args_min("chmod", args, 1)?;
            let path = get_path();
            let mode = args[0].to_int()? as u32;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(mode);
                std::fs::set_permissions(&path, perms)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            }
            #[cfg(not(unix))]
            {
                let _ = mode;
            }
            Ok(PyObject::none())
        }
        "match" => {
            check_args_min("match", args, 1)?;
            let path = get_path();
            let pattern = args[0].py_to_string();
            // Match against the full path or just the filename
            let p = std::path::Path::new(&path);
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let matched = simple_glob_match(&pattern, &name) || simple_glob_match(&pattern, &path);
            Ok(PyObject::bool_val(matched))
        }
        "samefile" => {
            check_args_min("samefile", args, 1)?;
            let path = get_path();
            let other = args[0].py_to_string();
            let meta1 = std::fs::canonicalize(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            let meta2 = std::fs::canonicalize(&other)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, other)))?;
            Ok(PyObject::bool_val(meta1 == meta2))
        }
        "rename" => {
            check_args_min("rename", args, 1)?;
            let path = get_path();
            let target = args[0].py_to_string();
            std::fs::rename(&path, &target)
                .map_err(|e| PyException::os_error(format!("{}: '{}' -> '{}'", e, path, target)))?;
            Ok(PyObject::str_val(CompactString::from(&target)))
        }
        "replace" => {
            check_args_min("replace", args, 1)?;
            let path = get_path();
            let target = args[0].py_to_string();
            // replace is like rename but silently replaces target if it exists
            std::fs::rename(&path, &target)
                .map_err(|e| PyException::os_error(format!("{}: '{}' -> '{}'", e, path, target)))?;
            Ok(PyObject::str_val(CompactString::from(&target)))
        }
        "open" => {
            // Simple open: return the text content for read mode
            let path = get_path();
            let mode = if !args.is_empty() {
                args[0].py_to_string()
            } else {
                "r".to_string()
            };
            if mode.contains('r') {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
                Ok(PyObject::str_val(CompactString::from(&content)))
            } else {
                // For write modes, create/truncate the file and return None
                std::fs::File::create(&path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
                Ok(PyObject::none())
            }
        }
        _ => Err(PyException::attribute_error(format!(
            "'Path' object has no attribute '{}'",
            method
        ))),
    }
}
