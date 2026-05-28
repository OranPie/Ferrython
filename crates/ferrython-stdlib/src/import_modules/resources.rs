use super::*;

// ── importlib.resources ──

pub fn create_importlib_resources_module() -> PyObjectRef {
    use std::path::PathBuf;

    // Helper: create a Traversable path object with read_bytes, read_text, joinpath
    fn make_traversable(pkg_path: String) -> PyObjectRef {
        let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("_path"),
                PyObject::str_val(CompactString::from(&pkg_path)),
            );

            let pp = pkg_path.clone();
            w.insert(
                CompactString::from("joinpath"),
                PyObject::native_closure("joinpath", move |a| {
                    let child = if !a.is_empty() {
                        a[0].py_to_string()
                    } else {
                        String::new()
                    };
                    let full = format!("{}/{}", pp, child);
                    Ok(make_traversable(full))
                }),
            );

            let pp2 = pkg_path.clone();
            w.insert(
                CompactString::from("__truediv__"),
                PyObject::native_closure("__truediv__", move |a| {
                    let child = if a.len() > 1 {
                        a[1].py_to_string()
                    } else if !a.is_empty() {
                        a[0].py_to_string()
                    } else {
                        String::new()
                    };
                    let full = format!("{}/{}", pp2, child);
                    Ok(make_traversable(full))
                }),
            );

            let pp3 = pkg_path.clone();
            w.insert(
                CompactString::from("read_bytes"),
                PyObject::native_closure("read_bytes", move |_| {
                    // Search site-packages for the path
                    let search = [
                        PathBuf::from(&pp3),
                        PathBuf::from(format!(
                            "target/release/lib/ferrython/site-packages/{}",
                            pp3
                        )),
                    ];
                    for p in &search {
                        if p.exists() {
                            match std::fs::read(p) {
                                Ok(data) => return Ok(PyObject::bytes(data)),
                                Err(e) => {
                                    return Err(PyException::os_error(format!(
                                        "cannot read {}: {}",
                                        p.display(),
                                        e
                                    )))
                                }
                            }
                        }
                    }
                    Err(PyException::file_not_found_error(format!(
                        "resource not found: {}",
                        pp3
                    )))
                }),
            );

            let pp4 = pkg_path.clone();
            w.insert(
                CompactString::from("read_text"),
                PyObject::native_closure("read_text", move |args| {
                    let encoding = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        "utf-8".to_string()
                    };
                    let _ = encoding; // always UTF-8 for now
                    let search = [
                        PathBuf::from(&pp4),
                        PathBuf::from(format!(
                            "target/release/lib/ferrython/site-packages/{}",
                            pp4
                        )),
                    ];
                    for p in &search {
                        if p.exists() {
                            match std::fs::read_to_string(p) {
                                Ok(data) => {
                                    return Ok(PyObject::str_val(CompactString::from(&data)))
                                }
                                Err(e) => {
                                    return Err(PyException::os_error(format!(
                                        "cannot read {}: {}",
                                        p.display(),
                                        e
                                    )))
                                }
                            }
                        }
                    }
                    Err(PyException::file_not_found_error(format!(
                        "resource not found: {}",
                        pp4
                    )))
                }),
            );

            let pp5 = pkg_path.clone();
            w.insert(
                CompactString::from("__str__"),
                PyObject::native_closure("__str__", move |_| {
                    Ok(PyObject::str_val(CompactString::from(&pp5)))
                }),
            );

            let pp6 = pkg_path;
            w.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from(pp6.rsplit('/').next().unwrap_or(&pp6))),
            );
        }
        inst
    }

    // files(package) — return a Traversable for the package directory
    let files_fn = make_builtin(|args: &[PyObjectRef]| {
        let pkg_name = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            String::new()
        };
        let pkg_path = pkg_name.replace('.', "/");
        Ok(make_traversable(pkg_path))
    });

    // read_text(package, resource, encoding='utf-8', errors='strict')
    let read_text_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.read_text", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let resource = args[1].py_to_string();
        let path = PathBuf::from(&pkg).join(&resource);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(PyObject::str_val(CompactString::from(&content))),
            Err(e) => Err(PyException::runtime_error(format!(
                "resource not found: {}",
                e
            ))),
        }
    });

    // read_binary(package, resource)
    let read_binary_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.read_binary", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let resource = args[1].py_to_string();
        let path = PathBuf::from(&pkg).join(&resource);
        match std::fs::read(&path) {
            Ok(data) => Ok(PyObject::bytes(data)),
            Err(e) => Err(PyException::runtime_error(format!(
                "resource not found: {}",
                e
            ))),
        }
    });

    // path(package, resource) — context manager yielding path
    let path_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.path", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let resource = args[1].py_to_string();
        let full = format!("{}/{}", pkg, resource);
        let path_obj = PyObject::str_val(CompactString::from(&full));
        // Wrap in a context manager
        let cls = PyObject::class(
            CompactString::from("_ResourcePath"),
            vec![],
            IndexMap::new(),
        );
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let p = path_obj.clone();
            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_| Ok(p.clone())),
            );
            w.insert(
                CompactString::from("__exit__"),
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            );
        }
        Ok(inst)
    });

    // is_resource(package, name)
    let is_resource_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.is_resource", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let name = args[1].py_to_string();
        let path = PathBuf::from(&pkg).join(&name);
        Ok(PyObject::bool_val(path.is_file()))
    });

    // contents(package)
    let contents_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.contents", args, 1)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let path = PathBuf::from(&pkg);
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                let names: Vec<PyObjectRef> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        PyObject::str_val(CompactString::from(
                            e.file_name().to_string_lossy().as_ref(),
                        ))
                    })
                    .collect();
                Ok(PyObject::list(names))
            }
            Err(_) => Ok(PyObject::list(vec![])),
        }
    });

    // as_file(traversable) — context manager that yields a path to the resource
    let as_file_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "as_file() requires a traversable argument",
            ));
        }
        let traversable = args[0].clone();
        // Get the path string from the traversable
        let path_str = traversable
            .get_attr("_path")
            .map(|p| p.py_to_string())
            .or_else(|| Some(traversable.py_to_string()))
            .unwrap_or_default();
        // Try to find the actual file in known locations
        let search = [
            PathBuf::from(&path_str),
            PathBuf::from(format!(
                "target/release/lib/ferrython/site-packages/{}",
                path_str
            )),
        ];
        let resolved = search
            .iter()
            .find(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path_str);
        let resolved_obj = PyObject::str_val(CompactString::from(&resolved));
        // Create a pathlib.Path-like context manager
        let cls = PyObject::class(CompactString::from("_AsFilePath"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let p = resolved_obj.clone();
            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_| {
                    // Return a pathlib.Path-like object
                    let path_cls =
                        PyObject::class(CompactString::from("PosixPath"), vec![], IndexMap::new());
                    let path_inst = PyObject::instance(path_cls);
                    if let PyObjectPayload::Instance(ref pd) = path_inst.payload {
                        let mut pw = pd.attrs.write();
                        pw.insert(CompactString::from("_path"), p.clone());
                        let ps = p.py_to_string();
                        pw.insert(
                            CompactString::from("__str__"),
                            PyObject::native_closure("__str__", move |_| {
                                Ok(PyObject::str_val(CompactString::from(&ps)))
                            }),
                        );
                        let ps2 = p.py_to_string();
                        pw.insert(
                            CompactString::from("__fspath__"),
                            PyObject::native_closure("__fspath__", move |_| {
                                Ok(PyObject::str_val(CompactString::from(&ps2)))
                            }),
                        );
                    }
                    Ok(path_inst)
                }),
            );
            w.insert(
                CompactString::from("__exit__"),
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            );
        }
        Ok(inst)
    });

    make_module(
        "importlib.resources",
        vec![
            ("files", files_fn),
            ("as_file", as_file_fn),
            ("read_text", read_text_fn),
            ("read_binary", read_binary_fn),
            ("path", path_fn),
            ("is_resource", is_resource_fn),
            ("contents", contents_fn),
        ],
    )
}
