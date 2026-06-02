use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

pub fn create_genericpath_module() -> PyObjectRef {
    make_module(
        "genericpath",
        vec![
            ("commonprefix", make_builtin(genericpath_commonprefix)),
            ("exists", make_builtin(genericpath_exists)),
            ("getatime", make_builtin(genericpath_getatime)),
            ("getctime", make_builtin(genericpath_getctime)),
            ("getmtime", make_builtin(genericpath_getmtime)),
            ("getsize", make_builtin(genericpath_getsize)),
            ("isdir", make_builtin(genericpath_isdir)),
            ("isfile", make_builtin(genericpath_isfile)),
            ("samefile", make_builtin(genericpath_samefile)),
            ("sameopenfile", make_builtin(genericpath_sameopenfile)),
            ("samestat", make_builtin(genericpath_samestat)),
            (
                "__all__",
                PyObject::list_leaf(
                    [
                        "commonprefix",
                        "exists",
                        "getatime",
                        "getctime",
                        "getmtime",
                        "getsize",
                        "isdir",
                        "isfile",
                        "samefile",
                        "sameopenfile",
                        "samestat",
                    ]
                    .into_iter()
                    .map(|name| PyObject::str_val(CompactString::from(name)))
                    .collect(),
                ),
            ),
        ],
    )
}

fn path_arg(name: &str, args: &[PyObjectRef]) -> PyResult<String> {
    check_args(name, args, 1)?;
    Ok(args[0].py_to_string())
}

fn metadata_for_path(path: &str) -> PyResult<std::fs::Metadata> {
    std::fs::metadata(path).map_err(|err| PyException::from_io_error(&err, Some(path)))
}

fn genericpath_exists(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.exists", args)?;
    Ok(PyObject::bool_val(std::fs::metadata(path).is_ok()))
}

fn genericpath_isfile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.isfile", args)?;
    Ok(PyObject::bool_val(
        std::fs::metadata(path)
            .map(|meta| meta.is_file())
            .unwrap_or(false),
    ))
}

fn genericpath_isdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.isdir", args)?;
    Ok(PyObject::bool_val(
        std::fs::metadata(path)
            .map(|meta| meta.is_dir())
            .unwrap_or(false),
    ))
}

fn genericpath_getsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.getsize", args)?;
    Ok(PyObject::int(metadata_for_path(&path)?.len() as i64))
}

fn genericpath_getmtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.getmtime", args)?;
    Ok(PyObject::float(metadata_time(
        &metadata_for_path(&path)?,
        TimeField::Modified,
    )))
}

fn genericpath_getatime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.getatime", args)?;
    Ok(PyObject::float(metadata_time(
        &metadata_for_path(&path)?,
        TimeField::Accessed,
    )))
}

fn genericpath_getctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = path_arg("genericpath.getctime", args)?;
    Ok(PyObject::float(metadata_time(
        &metadata_for_path(&path)?,
        TimeField::Changed,
    )))
}

enum TimeField {
    Accessed,
    Modified,
    Changed,
}

fn metadata_time(meta: &std::fs::Metadata, field: TimeField) -> f64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (secs, nanos) = match field {
            TimeField::Accessed => (meta.atime(), meta.atime_nsec()),
            TimeField::Modified => (meta.mtime(), meta.mtime_nsec()),
            TimeField::Changed => (meta.ctime(), meta.ctime_nsec()),
        };
        return secs as f64 + nanos as f64 / 1_000_000_000.0;
    }

    #[cfg(not(unix))]
    {
        let time = match field {
            TimeField::Accessed => meta.accessed(),
            TimeField::Modified => meta.modified(),
            TimeField::Changed => meta.created(),
        };
        time.ok()
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }
}

fn genericpath_commonprefix(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("genericpath.commonprefix", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }

    if is_bytes_like(&items[0]) {
        let mut byte_items = Vec::with_capacity(items.len());
        for item in &items {
            let Some(bytes) = bytes_value(item) else {
                return Err(PyException::type_error(
                    "can't mix str and bytes in genericpath.commonprefix",
                ));
            };
            byte_items.push(bytes);
        }
        byte_items.sort();
        let prefix = common_byte_prefix(&byte_items[0], byte_items.last().unwrap());
        return if matches!(&items[0].payload, PyObjectPayload::ByteArray(_)) {
            Ok(PyObject::bytearray(prefix))
        } else {
            Ok(PyObject::bytes(prefix))
        };
    }

    let mut text_items: Vec<String> = Vec::with_capacity(items.len());
    for item in &items {
        if is_bytes_like(item) {
            return Err(PyException::type_error(
                "can't mix str and bytes in genericpath.commonprefix",
            ));
        }
        text_items.push(item.py_to_string());
    }
    text_items.sort();
    let first = &text_items[0];
    let last = text_items.last().unwrap();
    let prefix = common_str_prefix(first, last);
    Ok(PyObject::str_val(CompactString::from(prefix)))
}

fn is_bytes_like(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
    )
}

fn bytes_value(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            Some((**bytes).clone())
        }
        _ => None,
    }
}

fn common_byte_prefix(first: &[u8], last: &[u8]) -> Vec<u8> {
    let limit = first.len().min(last.len());
    let mut end = 0;
    while end < limit && first[end] == last[end] {
        end += 1;
    }
    first[..end].to_vec()
}

fn common_str_prefix(first: &str, last: &str) -> String {
    let mut end = 0;
    for ((idx, left), right) in first.char_indices().zip(last.chars()) {
        if left != right {
            break;
        }
        end = idx + left.len_utf8();
    }
    first[..end].to_string()
}

fn genericpath_samefile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("genericpath.samefile", args, 2)?;
    let left = path_arg("genericpath.samefile", &args[0..1])?;
    let right = path_arg("genericpath.samefile", &args[1..2])?;
    let left_meta = metadata_for_path(&left)?;
    let right_meta = metadata_for_path(&right)?;
    Ok(PyObject::bool_val(
        metadata_identity(&left_meta) == metadata_identity(&right_meta),
    ))
}

fn genericpath_sameopenfile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("genericpath.sameopenfile", args, 2)?;
    let left = fd_identity(args[0].to_int()? as i32)?;
    let right = fd_identity(args[1].to_int()? as i32)?;
    Ok(PyObject::bool_val(left == right))
}

fn genericpath_samestat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("genericpath.samestat", args, 2)?;
    let left = stat_identity(&args[0])?;
    let right = stat_identity(&args[1])?;
    Ok(PyObject::bool_val(left == right))
}

#[cfg(unix)]
fn metadata_identity(meta: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn metadata_identity(meta: &std::fs::Metadata) -> (u64, u64) {
    (0, meta.len())
}

fn stat_identity(obj: &PyObjectRef) -> PyResult<(i64, i64)> {
    let dev = obj
        .get_attr("st_dev")
        .ok_or_else(|| PyException::attribute_error("stat result has no st_dev"))?
        .to_int()?;
    let ino = obj
        .get_attr("st_ino")
        .ok_or_else(|| PyException::attribute_error("stat result has no st_ino"))?
        .to_int()?;
    Ok((dev, ino))
}

#[cfg(unix)]
fn fd_identity(fd: i32) -> PyResult<(u64, u64)> {
    use std::os::unix::io::FromRawFd;
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let result = file.metadata();
    std::mem::forget(file);
    let meta = result.map_err(|err| PyException::from_io_error(&err, None))?;
    Ok(metadata_identity(&meta))
}

#[cfg(not(unix))]
fn fd_identity(_fd: i32) -> PyResult<(u64, u64)> {
    Err(PyException::not_implemented_error(
        "genericpath.sameopenfile not supported on this platform",
    ))
}
