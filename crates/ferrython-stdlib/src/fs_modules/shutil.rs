use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use super::glob_match;

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
