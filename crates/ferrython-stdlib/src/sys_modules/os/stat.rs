use super::*;

pub(super) fn build_stat_result_from_meta(meta: &std::fs::Metadata) -> PyResult<PyObjectRef> {
    let mut attrs = IndexMap::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        attrs.insert(
            CompactString::from("st_mode"),
            PyObject::int(meta.mode() as i64),
        );
        attrs.insert(
            CompactString::from("st_ino"),
            PyObject::int(meta.ino() as i64),
        );
        attrs.insert(
            CompactString::from("st_dev"),
            PyObject::int(meta.dev() as i64),
        );
        attrs.insert(
            CompactString::from("st_nlink"),
            PyObject::int(meta.nlink() as i64),
        );
        attrs.insert(
            CompactString::from("st_uid"),
            PyObject::int(meta.uid() as i64),
        );
        attrs.insert(
            CompactString::from("st_gid"),
            PyObject::int(meta.gid() as i64),
        );
    }
    #[cfg(not(unix))]
    {
        attrs.insert(CompactString::from("st_mode"), PyObject::int(0));
        attrs.insert(CompactString::from("st_ino"), PyObject::int(0));
        attrs.insert(CompactString::from("st_dev"), PyObject::int(0));
        attrs.insert(CompactString::from("st_nlink"), PyObject::int(0));
        attrs.insert(CompactString::from("st_uid"), PyObject::int(0));
        attrs.insert(CompactString::from("st_gid"), PyObject::int(0));
    }
    attrs.insert(
        CompactString::from("st_size"),
        PyObject::int(meta.len() as i64),
    );
    let epoch = std::time::SystemTime::UNIX_EPOCH;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let atime = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let ctime = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    attrs.insert(CompactString::from("st_mtime"), PyObject::float(mtime));
    attrs.insert(CompactString::from("st_atime"), PyObject::float(atime));
    attrs.insert(CompactString::from("st_ctime"), PyObject::float(ctime));
    Ok(PyObject::module_with_attrs(
        CompactString::from("os.stat_result"),
        attrs,
    ))
}

pub(super) fn os_stat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.stat", args, 1)?;
    let path = args[0].py_to_string();
    let meta = std::fs::metadata(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    build_stat_result_from_meta(&meta)
}

pub(super) fn os_lstat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("os.lstat requires path"));
    }
    let path = args[0].py_to_string();
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    crate::fs_modules::build_stat_result(meta)
}

pub(super) fn make_stat_result_class(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::class(
        CompactString::from("stat_result"),
        vec![],
        IndexMap::new(),
    ))
}

pub(super) fn os_scandir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() {
        ".".to_string()
    } else {
        args[0].py_to_string()
    };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = entry.path().to_string_lossy().to_string();
        let file_type = entry.file_type().ok();
        let is_file = file_type.as_ref().map(|ft| ft.is_file()).unwrap_or(false);
        let is_dir = file_type.as_ref().map(|ft| ft.is_dir()).unwrap_or(false);
        let is_symlink = file_type
            .as_ref()
            .map(|ft| ft.is_symlink())
            .unwrap_or(false);

        let cls = PyObject::class(CompactString::from("DirEntry"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(&name)),
        );
        attrs.insert(
            CompactString::from("path"),
            PyObject::str_val(CompactString::from(&full_path)),
        );

        let is_file_val = is_file;
        attrs.insert(
            CompactString::from("is_file"),
            PyObject::native_closure("DirEntry.is_file", move |_| {
                Ok(PyObject::bool_val(is_file_val))
            }),
        );
        let is_dir_val = is_dir;
        attrs.insert(
            CompactString::from("is_dir"),
            PyObject::native_closure("DirEntry.is_dir", move |_| {
                Ok(PyObject::bool_val(is_dir_val))
            }),
        );
        let is_sym_val = is_symlink;
        attrs.insert(
            CompactString::from("is_symlink"),
            PyObject::native_closure("DirEntry.is_symlink", move |_| {
                Ok(PyObject::bool_val(is_sym_val))
            }),
        );
        let stat_path = full_path.clone();
        attrs.insert(
            CompactString::from("stat"),
            PyObject::native_closure("DirEntry.stat", move |_| {
                let meta = std::fs::metadata(&stat_path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, stat_path)))?;
                crate::fs_modules::build_stat_result(meta)
            }),
        );
        let repr_name = name.clone();
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("DirEntry.__repr__", move |_| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "<DirEntry '{}'>",
                    repr_name
                ))))
            }),
        );
        let str_name = name.clone();
        attrs.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("DirEntry.__str__", move |_| {
                Ok(PyObject::str_val(CompactString::from(str_name.as_str())))
            }),
        );
        items.push(PyObject::instance_with_attrs(cls, attrs));
    }
    // Wrap in a ScandirIterator with context manager support
    let items_list = PyObject::list(items);
    let cls = PyObject::class(
        CompactString::from("ScandirIterator"),
        vec![],
        IndexMap::new(),
    );
    let mut attrs = IndexMap::new();
    let items_ref = items_list.clone();
    attrs.insert(CompactString::from("_entries"), items_list);
    attrs.insert(
        CompactString::from("__enter__"),
        PyObject::native_closure("ScandirIterator.__enter__", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("expected self"));
            }
            Ok(args[0].clone())
        }),
    );
    attrs.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("ScandirIterator.__exit__", move |_| Ok(PyObject::none())),
    );
    let iter_items = items_ref;
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("ScandirIterator.__iter__", move |_| {
            ferrython_core::object::PyObjectMethods::get_iter(&iter_items)
        }),
    );
    Ok(PyObject::instance_with_attrs(cls, attrs))
}
