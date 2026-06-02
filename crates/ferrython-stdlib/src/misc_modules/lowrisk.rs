use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_builtin, make_module, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

pub fn create_imghdr_module() -> PyObjectRef {
    make_module(
        "imghdr",
        vec![
            ("what", make_builtin(imghdr_what)),
            ("test_jpeg", make_builtin(|args| imghdr_test(args, "jpeg"))),
            ("test_png", make_builtin(|args| imghdr_test(args, "png"))),
            ("test_gif", make_builtin(|args| imghdr_test(args, "gif"))),
            ("test_tiff", make_builtin(|args| imghdr_test(args, "tiff"))),
            ("test_rgb", make_builtin(|args| imghdr_test(args, "rgb"))),
            ("test_pbm", make_builtin(|args| imghdr_test(args, "pbm"))),
            ("test_pgm", make_builtin(|args| imghdr_test(args, "pgm"))),
            ("test_ppm", make_builtin(|args| imghdr_test(args, "ppm"))),
            ("tests", PyObject::list(vec![])),
        ],
    )
}

pub fn create_sndhdr_module() -> PyObjectRef {
    make_module(
        "sndhdr",
        vec![
            ("what", make_builtin(sndhdr_what)),
            ("whathdr", make_builtin(sndhdr_whathdr)),
            (
                "SndHeaders",
                PyObject::builtin_type(CompactString::from("tuple")),
            ),
        ],
    )
}

pub fn create_nturl2path_module() -> PyObjectRef {
    make_module(
        "nturl2path",
        vec![
            ("url2pathname", make_builtin(nturl2path_url2pathname)),
            ("pathname2url", make_builtin(nturl2path_pathname2url)),
        ],
    )
}

pub fn create_filecmp_module() -> PyObjectRef {
    let dircmp = make_dircmp_class();
    make_module(
        "filecmp",
        vec![
            ("cmp", make_builtin(filecmp_cmp)),
            ("cmpfiles", make_builtin(filecmp_cmpfiles)),
            ("dircmp", dircmp),
            (
                "DEFAULT_IGNORES",
                PyObject::list(
                    [
                        "RCS",
                        "CVS",
                        "tags",
                        ".git",
                        ".hg",
                        ".bzr",
                        "_darcs",
                        "__pycache__",
                    ]
                    .into_iter()
                    .map(|item| PyObject::str_val(CompactString::from(item)))
                    .collect(),
                ),
            ),
        ],
    )
}

pub fn create_chunk_module() -> PyObjectRef {
    make_module("chunk", vec![("Chunk", make_chunk_class())])
}

pub fn create_xdrlib_module() -> PyObjectRef {
    let error = PyObject::class(
        CompactString::from("Error"),
        vec![PyObject::exception_type(ExceptionKind::Exception)],
        IndexMap::new(),
    );
    let conversion_error = PyObject::class(
        CompactString::from("ConversionError"),
        vec![error.clone()],
        IndexMap::new(),
    );
    make_module(
        "xdrlib",
        vec![
            ("Error", error),
            ("ConversionError", conversion_error),
            ("Packer", make_xdr_packer_class()),
            ("Unpacker", make_xdr_unpacker_class()),
        ],
    )
}

pub fn create_uu_module() -> PyObjectRef {
    make_module(
        "uu",
        vec![
            ("Error", PyObject::exception_type(ExceptionKind::Exception)),
            ("encode", make_builtin(uu_encode)),
            ("decode", make_builtin(uu_decode)),
            ("_uu_char", make_builtin(uu_char_fn)),
            ("_uu_value", make_builtin(uu_value_fn)),
            ("_uu_encode_chunk", make_builtin(uu_encode_chunk_fn)),
            ("_uu_decode_line", make_builtin(uu_decode_line_fn)),
        ],
    )
}

fn bytes_from_obj(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some((**b).clone()),
        PyObjectPayload::Str(s) => Some(s.as_str().as_bytes().to_vec()),
        _ => None,
    }
}

fn read_file_or_object(obj: &PyObjectRef, size: Option<usize>) -> PyResult<PyObjectRef> {
    if let Some(read) = obj.get_attr("read") {
        let args = size
            .map(|n| vec![PyObject::int(n as i64)])
            .unwrap_or_default();
        return call_callable(&read, &args);
    }
    let path = obj.py_to_string();
    let mut data = std::fs::read(&path).map_err(|e| PyException::from_io_error(&e, Some(&path)))?;
    if let Some(n) = size {
        data.truncate(n);
    }
    Ok(PyObject::bytes(data))
}

fn write_file_or_object(target: &PyObjectRef, data: PyObjectRef, binary: bool) -> PyResult<()> {
    if let Some(write) = target.get_attr("write") {
        let _ = call_callable(&write, &[data])?;
        return Ok(());
    }
    let path = target.py_to_string();
    if binary {
        let bytes = bytes_from_obj(&data).unwrap_or_else(|| data.py_to_string().into_bytes());
        std::fs::write(&path, bytes).map_err(|e| PyException::from_io_error(&e, Some(&path)))?;
    } else {
        std::fs::write(&path, data.py_to_string())
            .map_err(|e| PyException::from_io_error(&e, Some(&path)))?;
    }
    Ok(())
}

fn call_noarg(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    obj.get_attr(name)
        .and_then(|method| call_callable(&method, &[]).ok())
}

fn call_onearg(obj: &PyObjectRef, name: &str, arg: PyObjectRef) -> PyResult<PyObjectRef> {
    let method = obj
        .get_attr(name)
        .ok_or_else(|| PyException::attribute_error(name))?;
    call_callable(&method, &[arg])
}

fn seek_file(obj: &PyObjectRef, pos: i64, whence: Option<i64>) -> PyResult<()> {
    let method = obj
        .get_attr("seek")
        .ok_or_else(|| PyException::attribute_error("seek"))?;
    let mut args = vec![PyObject::int(pos)];
    if let Some(whence) = whence {
        args.push(PyObject::int(whence));
    }
    let _ = call_callable(&method, &args)?;
    Ok(())
}

fn imghdr_what(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("what() requires a file"));
    }
    let header = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::None) {
        bytes_from_obj(&args[1]).unwrap_or_else(|| args[1].py_to_string().into_bytes())
    } else {
        read_file_or_object(&args[0], Some(32))
            .map(|obj| bytes_from_obj(&obj).unwrap_or_else(|| obj.py_to_string().into_bytes()))?
    };
    match image_type(&header) {
        Some(kind) => Ok(PyObject::str_val(CompactString::from(kind))),
        None => Ok(PyObject::none()),
    }
}

fn imghdr_test(args: &[PyObjectRef], expected: &'static str) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("image test requires header"));
    }
    let header = bytes_from_obj(&args[0]).unwrap_or_else(|| args[0].py_to_string().into_bytes());
    if image_type(&header) == Some(expected) {
        Ok(PyObject::str_val(CompactString::from(expected)))
    } else {
        Ok(PyObject::none())
    }
}

fn image_type(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(&[0xff, 0xd8, 0xff]) || matches!(h.get(6..10), Some(b"JFIF" | b"Exif")) {
        return Some("jpeg");
    }
    if h.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if h.starts_with(b"GIF87a") || h.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if h.starts_with(b"MM") || h.starts_with(b"II") {
        return Some("tiff");
    }
    if h.starts_with(&[1, 0xda]) {
        return Some("rgb");
    }
    if h.len() >= 3 && h[0] == b'P' && matches!(h[2], b' ' | b'\t' | b'\n' | b'\r') {
        return match h[1] {
            b'1' | b'4' => Some("pbm"),
            b'2' | b'5' => Some("pgm"),
            b'3' | b'6' => Some("ppm"),
            _ => None,
        };
    }
    None
}

fn sndhdr_what(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("what() requires a filename"));
    }
    let header = read_file_or_object(&args[0], Some(512))
        .map(|obj| bytes_from_obj(&obj).unwrap_or_else(|| obj.py_to_string().into_bytes()))?;
    sound_header_obj(sound_type(&header))
}

fn sndhdr_whathdr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sndhdr.whathdr", args, 2)?;
    let header = bytes_from_obj(&args[1]).unwrap_or_else(|| args[1].py_to_string().into_bytes());
    sound_header_obj(sound_type(&header))
}

fn sound_type(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(b"RIFF") && matches!(h.get(8..12), Some(b"WAVE")) {
        Some("wav")
    } else if h.starts_with(b"FORM") && matches!(h.get(8..12), Some(b"AIFF" | b"AIFC")) {
        Some("aiff")
    } else if h.starts_with(b".snd") {
        Some("au")
    } else if h.starts_with(b"ID3") || (h.len() > 2 && h[0] == 255 && (h[1] & 224) == 224) {
        Some("mp3")
    } else {
        None
    }
}

fn sound_header_obj(kind: Option<&'static str>) -> PyResult<PyObjectRef> {
    Ok(match kind {
        Some(kind) => PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(kind)),
            PyObject::none(),
            PyObject::none(),
            PyObject::none(),
            PyObject::none(),
        ]),
        None => PyObject::none(),
    })
}

fn nturl2path_url2pathname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("nturl2path.url2pathname", args, 1)?;
    let mut url = percent_decode(&args[0].py_to_string());
    if url.starts_with("///") {
        url = url[2..].to_string();
    } else if url.starts_with("//") {
        let rest = &url[2..];
        let mut split = rest.splitn(2, '/');
        let host = split.next().unwrap_or("");
        let tail = split.next().unwrap_or("").replace('/', "\\");
        return Ok(PyObject::str_val(CompactString::from(format!(
            "\\\\{}\\{}",
            host, tail
        ))));
    }
    if url.len() >= 3 && url.starts_with('/') {
        let bytes = url.as_bytes();
        if bytes[2] == b'|' || bytes[2] == b':' {
            url = format!("{}:{}", &url[1..2], &url[3..]);
        }
    }
    Ok(PyObject::str_val(CompactString::from(
        url.replace('/', "\\"),
    )))
}

fn nturl2path_pathname2url(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("nturl2path.pathname2url", args, 1)?;
    let path = args[0].py_to_string().replace('\\', "/");
    let encoded = if let Some(rest) = path.strip_prefix("//") {
        format!("//{}", percent_encode(rest))
    } else if path.len() >= 2 && path.as_bytes()[1] == b':' {
        format!("/{}:{}", &path[..1], percent_encode(&path[2..]))
    } else {
        percent_encode(&path)
    };
    Ok(PyObject::str_val(CompactString::from(encoded)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

fn filecmp_cmp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("cmp() requires two files"));
    }
    let left = args[0].py_to_string();
    let right = args[1].py_to_string();
    let shallow = args.get(2).is_none_or(|obj| obj.is_truthy());
    Ok(PyObject::bool_val(compare_files(&left, &right, shallow)?))
}

fn compare_files(left: &str, right: &str, shallow: bool) -> PyResult<bool> {
    let lm = std::fs::metadata(left).map_err(|e| PyException::from_io_error(&e, Some(left)))?;
    let rm = std::fs::metadata(right).map_err(|e| PyException::from_io_error(&e, Some(right)))?;
    if lm.len() != rm.len() {
        return Ok(false);
    }
    if shallow {
        let lt = lm.modified().ok();
        let rt = rm.modified().ok();
        if lt == rt {
            return Ok(true);
        }
    }
    let lb = std::fs::read(left).map_err(|e| PyException::from_io_error(&e, Some(left)))?;
    let rb = std::fs::read(right).map_err(|e| PyException::from_io_error(&e, Some(right)))?;
    Ok(lb == rb)
}

fn filecmp_cmpfiles(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error("cmpfiles() requires a, b, common"));
    }
    let left_dir = args[0].py_to_string();
    let right_dir = args[1].py_to_string();
    let shallow = args.get(3).is_none_or(|obj| obj.is_truthy());
    let mut matches = Vec::new();
    let mut mismatch = Vec::new();
    let mut errors = Vec::new();
    for name_obj in args[2].to_list()? {
        let name = name_obj.py_to_string();
        let left = format!("{}/{}", left_dir.trim_end_matches('/'), name);
        let right = format!("{}/{}", right_dir.trim_end_matches('/'), name);
        match compare_files(&left, &right, shallow) {
            Ok(true) => matches.push(PyObject::str_val(CompactString::from(name))),
            Ok(false) => mismatch.push(PyObject::str_val(CompactString::from(name))),
            Err(_) => errors.push(PyObject::str_val(CompactString::from(name))),
        }
    }
    Ok(PyObject::tuple(vec![
        PyObject::list(matches),
        PyObject::list(mismatch),
        PyObject::list(errors),
    ]))
}

fn make_dircmp_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("dircmp.__init__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("dircmp requires left and right"));
            }
            let self_obj = &args[0];
            let left = args[1].py_to_string();
            let right = args[2].py_to_string();
            let ignore = args.get(3).map_or_else(
                || vec![".".to_string(), "..".to_string()],
                |obj| {
                    obj.to_list()
                        .unwrap_or_default()
                        .iter()
                        .map(|x| x.py_to_string())
                        .collect()
                },
            );
            let hide = args.get(4).map_or_else(
                || vec![".".to_string(), "..".to_string()],
                |obj| {
                    obj.to_list()
                        .unwrap_or_default()
                        .iter()
                        .map(|x| x.py_to_string())
                        .collect()
                },
            );
            set_instance_attr(
                self_obj,
                "left",
                PyObject::str_val(CompactString::from(left)),
            )?;
            set_instance_attr(
                self_obj,
                "right",
                PyObject::str_val(CompactString::from(right)),
            )?;
            set_instance_attr(
                self_obj,
                "ignore",
                PyObject::list(
                    ignore
                        .into_iter()
                        .map(|x| PyObject::str_val(CompactString::from(x)))
                        .collect(),
                ),
            )?;
            set_instance_attr(
                self_obj,
                "hide",
                PyObject::list(
                    hide.into_iter()
                        .map(|x| PyObject::str_val(CompactString::from(x)))
                        .collect(),
                ),
            )?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("report"),
        PyObject::native_function("dircmp.report", |_| Ok(PyObject::none())),
    );
    PyObject::class(CompactString::from("dircmp"), vec![], ns)
}

fn set_instance_attr(obj: &PyObjectRef, name: &str, value: PyObjectRef) -> PyResult<()> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.write().insert(CompactString::from(name), value);
        Ok(())
    } else {
        Err(PyException::type_error("expected instance"))
    }
}

fn get_instance_attr(obj: &PyObjectRef, name: &str) -> PyResult<PyObjectRef> {
    obj.get_attr(name)
        .ok_or_else(|| PyException::attribute_error(name))
}

fn make_chunk_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("Chunk.__init__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("Chunk requires file"));
            }
            let self_obj = &args[0];
            let file = args[1].clone();
            let align = args.get(2).is_none_or(|obj| obj.is_truthy());
            let bigendian = args.get(3).is_none_or(|obj| obj.is_truthy());
            let inclheader = args.get(4).is_some_and(|obj| obj.is_truthy());
            let name = read_from_file(&file, 4)?;
            if name.len() < 4 {
                return Err(PyException::new(ExceptionKind::EOFError, ""));
            }
            let raw = read_from_file(&file, 4)?;
            if raw.len() < 4 {
                return Err(PyException::new(ExceptionKind::EOFError, ""));
            }
            let mut size = if bigendian {
                u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64
            } else {
                u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64
            };
            if inclheader {
                size -= 8;
            }
            let offset = call_noarg(&file, "tell").unwrap_or_else(PyObject::none);
            set_instance_attr(self_obj, "file", file)?;
            set_instance_attr(self_obj, "align", PyObject::bool_val(align))?;
            set_instance_attr(self_obj, "closed", PyObject::bool_val(false))?;
            set_instance_attr(self_obj, "size_read", PyObject::int(0))?;
            set_instance_attr(self_obj, "chunkname", PyObject::bytes(name))?;
            set_instance_attr(self_obj, "chunksize", PyObject::int(size))?;
            set_instance_attr(self_obj, "offset", offset)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("getname"),
        PyObject::native_function("Chunk.getname", chunk_getname),
    );
    ns.insert(
        CompactString::from("getsize"),
        PyObject::native_function("Chunk.getsize", chunk_getsize),
    );
    ns.insert(
        CompactString::from("read"),
        PyObject::native_function("Chunk.read", chunk_read),
    );
    ns.insert(
        CompactString::from("skip"),
        PyObject::native_function("Chunk.skip", chunk_skip),
    );
    ns.insert(
        CompactString::from("close"),
        PyObject::native_function("Chunk.close", chunk_close),
    );
    ns.insert(
        CompactString::from("seek"),
        PyObject::native_function("Chunk.seek", chunk_seek),
    );
    ns.insert(
        CompactString::from("tell"),
        PyObject::native_function("Chunk.tell", chunk_tell),
    );
    PyObject::class(CompactString::from("Chunk"), vec![], ns)
}

fn read_from_file(file: &PyObjectRef, size: usize) -> PyResult<Vec<u8>> {
    let obj = call_onearg(file, "read", PyObject::int(size as i64))?;
    Ok(bytes_from_obj(&obj).unwrap_or_else(|| obj.py_to_string().into_bytes()))
}

fn chunk_self(args: &[PyObjectRef]) -> PyResult<&PyObjectRef> {
    args.first()
        .ok_or_else(|| PyException::type_error("chunk method requires self"))
}

fn chunk_getname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    get_instance_attr(chunk_self(args)?, "chunkname")
}

fn chunk_getsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    get_instance_attr(chunk_self(args)?, "chunksize")
}

fn chunk_tell(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    get_instance_attr(chunk_self(args)?, "size_read")
}

fn chunk_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let self_obj = chunk_self(args)?;
    if get_instance_attr(self_obj, "closed")?.is_truthy() {
        return Err(PyException::value_error("I/O operation on closed file"));
    }
    let chunksize = get_instance_attr(self_obj, "chunksize")?.to_int()?;
    let size_read = get_instance_attr(self_obj, "size_read")?.to_int()?;
    let remaining = (chunksize - size_read).max(0);
    let mut size = args.get(1).map_or(-1, |obj| obj.to_int().unwrap_or(-1));
    if size < 0 || size > remaining {
        size = remaining;
    }
    let file = get_instance_attr(self_obj, "file")?;
    let data = read_from_file(&file, size as usize)?;
    let new_read = size_read + data.len() as i64;
    set_instance_attr(self_obj, "size_read", PyObject::int(new_read))?;
    if new_read == chunksize
        && get_instance_attr(self_obj, "align")?.is_truthy()
        && chunksize % 2 != 0
    {
        let _ = read_from_file(&file, 1);
    }
    Ok(PyObject::bytes(data))
}

fn chunk_skip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let self_obj = chunk_self(args)?;
    if get_instance_attr(self_obj, "closed")?.is_truthy() {
        return Err(PyException::value_error("I/O operation on closed file"));
    }
    let chunksize = get_instance_attr(self_obj, "chunksize")?.to_int()?;
    let size_read = get_instance_attr(self_obj, "size_read")?.to_int()?;
    let remaining = (chunksize - size_read).max(0);
    let file = get_instance_attr(self_obj, "file")?;
    if remaining > 0 {
        if seek_file(&file, remaining, Some(1)).is_err() {
            let _ = read_from_file(&file, remaining as usize)?;
        }
        set_instance_attr(self_obj, "size_read", PyObject::int(chunksize))?;
    }
    if get_instance_attr(self_obj, "align")?.is_truthy() && chunksize % 2 != 0 {
        let _ = read_from_file(&file, 1);
    }
    Ok(PyObject::none())
}

fn chunk_close(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let self_obj = chunk_self(args)?.clone();
    if !get_instance_attr(&self_obj, "closed")?.is_truthy() {
        let _ = chunk_skip(&[self_obj.clone()])?;
        set_instance_attr(&self_obj, "closed", PyObject::bool_val(true))?;
    }
    Ok(PyObject::none())
}

fn chunk_seek(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("seek requires position"));
    }
    let self_obj = &args[0];
    let offset = get_instance_attr(self_obj, "offset")?;
    if matches!(offset.payload, PyObjectPayload::None) {
        return Err(PyException::os_error("cannot seek"));
    }
    let pos = args[1].to_int()?;
    let whence = args.get(2).map_or(0, |obj| obj.to_int().unwrap_or(0));
    let size_read = get_instance_attr(self_obj, "size_read")?.to_int()?;
    let chunksize = get_instance_attr(self_obj, "chunksize")?.to_int()?;
    let target = match whence {
        0 => pos,
        1 => size_read + pos,
        2 => chunksize + pos,
        _ => return Err(PyException::value_error("invalid whence")),
    };
    if target < 0 || target > chunksize {
        return Err(PyException::runtime_error("seek out of range"));
    }
    let file = get_instance_attr(self_obj, "file")?;
    seek_file(&file, offset.to_int()? + target, None)?;
    set_instance_attr(self_obj, "size_read", PyObject::int(target))?;
    Ok(PyObject::none())
}

fn make_xdr_packer_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("Packer.__init__", |args| {
            let self_obj = args
                .first()
                .ok_or_else(|| PyException::type_error("Packer.__init__ requires self"))?;
            set_instance_attr(self_obj, "_buf", PyObject::bytearray(Vec::new()))?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("reset"),
        PyObject::native_function("Packer.reset", xdr_packer_reset),
    );
    ns.insert(
        CompactString::from("get_buffer"),
        PyObject::native_function("Packer.get_buffer", xdr_packer_get_buffer),
    );
    ns.insert(
        CompactString::from("pack_uint"),
        PyObject::native_function("Packer.pack_uint", xdr_pack_uint),
    );
    ns.insert(
        CompactString::from("pack_enum"),
        PyObject::native_function("Packer.pack_enum", xdr_pack_uint),
    );
    ns.insert(
        CompactString::from("pack_int"),
        PyObject::native_function("Packer.pack_int", xdr_pack_int),
    );
    ns.insert(
        CompactString::from("pack_bool"),
        PyObject::native_function("Packer.pack_bool", xdr_pack_bool),
    );
    ns.insert(
        CompactString::from("pack_uhyper"),
        PyObject::native_function("Packer.pack_uhyper", xdr_pack_uhyper),
    );
    ns.insert(
        CompactString::from("pack_hyper"),
        PyObject::native_function("Packer.pack_hyper", xdr_pack_hyper),
    );
    ns.insert(
        CompactString::from("pack_fstring"),
        PyObject::native_function("Packer.pack_fstring", xdr_pack_fstring),
    );
    ns.insert(
        CompactString::from("pack_string"),
        PyObject::native_function("Packer.pack_string", xdr_pack_string),
    );
    ns.insert(
        CompactString::from("pack_bytes"),
        PyObject::native_function("Packer.pack_bytes", xdr_pack_string),
    );
    ns.insert(
        CompactString::from("pack_opaque"),
        PyObject::native_function("Packer.pack_opaque", xdr_pack_string),
    );
    ns.insert(
        CompactString::from("pack_list"),
        PyObject::native_function("Packer.pack_list", xdr_pack_list),
    );
    ns.insert(
        CompactString::from("pack_array"),
        PyObject::native_function("Packer.pack_array", xdr_pack_array),
    );
    PyObject::class(CompactString::from("Packer"), vec![], ns)
}

fn xdr_buf(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    get_instance_attr(obj, "_buf").map(|buf| bytes_from_obj(&buf).unwrap_or_default())
}

fn set_xdr_buf(obj: &PyObjectRef, buf: Vec<u8>) -> PyResult<()> {
    set_instance_attr(obj, "_buf", PyObject::bytearray(buf))
}

fn xdr_append(obj: &PyObjectRef, bytes: &[u8]) -> PyResult<()> {
    let mut buf = xdr_buf(obj)?;
    buf.extend_from_slice(bytes);
    set_xdr_buf(obj, buf)
}

fn xdr_packer_reset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    set_xdr_buf(chunk_self(args)?, Vec::new())?;
    Ok(PyObject::none())
}

fn xdr_packer_get_buffer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bytes(xdr_buf(chunk_self(args)?)?))
}

fn xdr_pack_uint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("pack_uint requires value"));
    }
    let value = args[1].to_int()? as u32;
    xdr_append(&args[0], &value.to_be_bytes())?;
    Ok(PyObject::none())
}

fn xdr_pack_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("pack_int requires value"));
    }
    let value = args[1].to_int()? as i32;
    xdr_append(&args[0], &value.to_be_bytes())?;
    Ok(PyObject::none())
}

fn xdr_pack_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("pack_bool requires value"));
    }
    let value = if args[1].is_truthy() { 1u32 } else { 0u32 };
    xdr_append(&args[0], &value.to_be_bytes())?;
    Ok(PyObject::none())
}

fn xdr_pack_uhyper(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("pack_uhyper requires value"));
    }
    let value = args[1].to_int()? as u64;
    xdr_append(&args[0], &value.to_be_bytes())?;
    Ok(PyObject::none())
}

fn xdr_pack_hyper(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("pack_hyper requires value"));
    }
    let value = args[1].to_int()?;
    xdr_append(&args[0], &value.to_be_bytes())?;
    Ok(PyObject::none())
}

fn xdr_pack_fstring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error("pack_fstring requires n and s"));
    }
    let n = args[1].to_int()? as usize;
    let data = bytes_from_obj(&args[2]).unwrap_or_else(|| args[2].py_to_string().into_bytes());
    if data.len() != n {
        return Err(PyException::value_error("fstring size mismatch"));
    }
    let pad = (4 - n % 4) % 4;
    xdr_append(&args[0], &data)?;
    if pad > 0 {
        xdr_append(&args[0], &vec![0; pad])?;
    }
    Ok(PyObject::none())
}

fn xdr_pack_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("pack_string requires data"));
    }
    let data = bytes_from_obj(&args[1]).unwrap_or_else(|| args[1].py_to_string().into_bytes());
    xdr_append(&args[0], &(data.len() as u32).to_be_bytes())?;
    let pad = (4 - data.len() % 4) % 4;
    xdr_append(&args[0], &data)?;
    if pad > 0 {
        xdr_append(&args[0], &vec![0; pad])?;
    }
    Ok(PyObject::none())
}

fn xdr_pack_list(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "pack_list requires items and pack_item",
        ));
    }
    for item in args[1].to_list()? {
        xdr_append(&args[0], &1u32.to_be_bytes())?;
        let _ = call_callable(&args[2], &[item])?;
    }
    xdr_append(&args[0], &0u32.to_be_bytes())?;
    Ok(PyObject::none())
}

fn xdr_pack_array(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "pack_array requires items and pack_item",
        ));
    }
    let items = args[1].to_list()?;
    xdr_append(&args[0], &(items.len() as u32).to_be_bytes())?;
    for item in items {
        let _ = call_callable(&args[2], &[item])?;
    }
    Ok(PyObject::none())
}

fn make_xdr_unpacker_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("Unpacker.__init__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("Unpacker requires data"));
            }
            let self_obj = &args[0];
            let data =
                bytes_from_obj(&args[1]).unwrap_or_else(|| args[1].py_to_string().into_bytes());
            set_instance_attr(self_obj, "_buf", PyObject::bytes(data))?;
            set_instance_attr(self_obj, "_pos", PyObject::int(0))?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("reset"),
        PyObject::native_function("Unpacker.reset", xdr_unpacker_reset),
    );
    ns.insert(
        CompactString::from("get_position"),
        PyObject::native_function("Unpacker.get_position", xdr_unpacker_get_position),
    );
    ns.insert(
        CompactString::from("set_position"),
        PyObject::native_function("Unpacker.set_position", xdr_unpacker_set_position),
    );
    ns.insert(
        CompactString::from("done"),
        PyObject::native_function("Unpacker.done", xdr_unpacker_done),
    );
    ns.insert(
        CompactString::from("unpack_uint"),
        PyObject::native_function("Unpacker.unpack_uint", xdr_unpack_uint),
    );
    ns.insert(
        CompactString::from("unpack_enum"),
        PyObject::native_function("Unpacker.unpack_enum", xdr_unpack_uint),
    );
    ns.insert(
        CompactString::from("unpack_int"),
        PyObject::native_function("Unpacker.unpack_int", xdr_unpack_int),
    );
    ns.insert(
        CompactString::from("unpack_bool"),
        PyObject::native_function("Unpacker.unpack_bool", xdr_unpack_bool),
    );
    ns.insert(
        CompactString::from("unpack_uhyper"),
        PyObject::native_function("Unpacker.unpack_uhyper", xdr_unpack_uhyper),
    );
    ns.insert(
        CompactString::from("unpack_hyper"),
        PyObject::native_function("Unpacker.unpack_hyper", xdr_unpack_hyper),
    );
    ns.insert(
        CompactString::from("unpack_fstring"),
        PyObject::native_function("Unpacker.unpack_fstring", xdr_unpack_fstring),
    );
    ns.insert(
        CompactString::from("unpack_string"),
        PyObject::native_function("Unpacker.unpack_string", xdr_unpack_string),
    );
    ns.insert(
        CompactString::from("unpack_bytes"),
        PyObject::native_function("Unpacker.unpack_bytes", xdr_unpack_string),
    );
    ns.insert(
        CompactString::from("unpack_opaque"),
        PyObject::native_function("Unpacker.unpack_opaque", xdr_unpack_string),
    );
    ns.insert(
        CompactString::from("unpack_list"),
        PyObject::native_function("Unpacker.unpack_list", xdr_unpack_list),
    );
    ns.insert(
        CompactString::from("unpack_array"),
        PyObject::native_function("Unpacker.unpack_array", xdr_unpack_array),
    );
    PyObject::class(CompactString::from("Unpacker"), vec![], ns)
}

fn xdr_unpack_state(obj: &PyObjectRef) -> PyResult<(Vec<u8>, usize)> {
    let buf = bytes_from_obj(&get_instance_attr(obj, "_buf")?).unwrap_or_default();
    let pos = get_instance_attr(obj, "_pos")?.to_int()? as usize;
    Ok((buf, pos))
}

fn set_xdr_pos(obj: &PyObjectRef, pos: usize) -> PyResult<()> {
    set_instance_attr(obj, "_pos", PyObject::int(pos as i64))
}

fn xdr_read(obj: &PyObjectRef, n: usize) -> PyResult<Vec<u8>> {
    let (buf, pos) = xdr_unpack_state(obj)?;
    if pos + n > buf.len() {
        return Err(PyException::new(ExceptionKind::EOFError, ""));
    }
    let data = buf[pos..pos + n].to_vec();
    set_xdr_pos(obj, pos + n)?;
    Ok(data)
}

fn xdr_unpacker_reset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("reset requires data"));
    }
    let data = bytes_from_obj(&args[1]).unwrap_or_else(|| args[1].py_to_string().into_bytes());
    set_instance_attr(&args[0], "_buf", PyObject::bytes(data))?;
    set_xdr_pos(&args[0], 0)?;
    Ok(PyObject::none())
}

fn xdr_unpacker_get_position(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    get_instance_attr(chunk_self(args)?, "_pos")
}

fn xdr_unpacker_set_position(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("set_position requires pos"));
    }
    set_xdr_pos(&args[0], args[1].to_int()? as usize)?;
    Ok(PyObject::none())
}

fn xdr_unpacker_done(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (buf, pos) = xdr_unpack_state(chunk_self(args)?)?;
    if pos != buf.len() {
        return Err(PyException::with_original(
            ExceptionKind::Exception,
            "unextracted data remains",
            PyObject::exception_instance(ExceptionKind::Exception, "unextracted data remains"),
        ));
    }
    Ok(PyObject::none())
}

fn xdr_unpack_uint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = xdr_read(chunk_self(args)?, 4)?;
    Ok(PyObject::int(
        u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as i64,
    ))
}

fn xdr_unpack_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = xdr_read(chunk_self(args)?, 4)?;
    Ok(PyObject::int(
        i32::from_be_bytes([data[0], data[1], data[2], data[3]]) as i64,
    ))
}

fn xdr_unpack_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(xdr_unpack_uint(args)?.to_int()? != 0))
}

fn xdr_unpack_uhyper(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = xdr_read(chunk_self(args)?, 8)?;
    let value = u64::from_be_bytes(data.try_into().unwrap());
    Ok(PyObject::int(value as i64))
}

fn xdr_unpack_hyper(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = xdr_read(chunk_self(args)?, 8)?;
    Ok(PyObject::int(i64::from_be_bytes(data.try_into().unwrap())))
}

fn xdr_unpack_fstring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("unpack_fstring requires n"));
    }
    let n = args[1].to_int()? as usize;
    let data = xdr_read(&args[0], n)?;
    let pad = (4 - n % 4) % 4;
    if pad > 0 {
        let _ = xdr_read(&args[0], pad)?;
    }
    Ok(PyObject::bytes(data))
}

fn xdr_unpack_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let len = xdr_unpack_uint(args)?.to_int()? as usize;
    xdr_unpack_fstring(&[args[0].clone(), PyObject::int(len as i64)])
}

fn xdr_unpack_list(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("unpack_list requires unpack_item"));
    }
    let mut items = Vec::new();
    loop {
        if xdr_unpack_uint(&[args[0].clone()])?.to_int()? == 0 {
            break;
        }
        items.push(call_callable(&args[1], &[])?);
    }
    Ok(PyObject::list(items))
}

fn xdr_unpack_array(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("unpack_array requires unpack_item"));
    }
    let n = xdr_unpack_uint(&[args[0].clone()])?.to_int()? as usize;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(call_callable(&args[1], &[])?);
    }
    Ok(PyObject::list(items))
}

const UU_CHARS: &[u8] = b" !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_";

fn uu_char(value: u8) -> char {
    UU_CHARS[(value & 0x3f) as usize] as char
}

fn uu_value_byte(ch: u8) -> u8 {
    ch.wrapping_sub(32) & 0x3f
}

fn uu_char_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("uu._uu_char", args, 1)?;
    Ok(PyObject::str_val(CompactString::from(
        uu_char(args[0].to_int()? as u8).to_string(),
    )))
}

fn uu_value_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("uu._uu_value", args, 1)?;
    let raw = args[0].py_to_string();
    let ch = raw.as_bytes().first().copied().unwrap_or(0);
    Ok(PyObject::int(uu_value_byte(ch) as i64))
}

fn uu_encode_chunk(chunk: &[u8]) -> String {
    let mut out = String::new();
    out.push(uu_char(chunk.len() as u8));
    for triple in chunk.chunks(3) {
        let b1 = triple.first().copied().unwrap_or(0);
        let b2 = triple.get(1).copied().unwrap_or(0);
        let b3 = triple.get(2).copied().unwrap_or(0);
        out.push(uu_char(b1 >> 2));
        out.push(uu_char(((b1 << 4) | (b2 >> 4)) & 0x3f));
        out.push(uu_char(((b2 << 2) | (b3 >> 6)) & 0x3f));
        out.push(uu_char(b3 & 0x3f));
    }
    out.push('\n');
    out
}

fn uu_decode_line_bytes(line: &str) -> Vec<u8> {
    if line.is_empty() {
        return Vec::new();
    }
    let bytes = line.as_bytes();
    let len = uu_value_byte(bytes[0]) as usize;
    let body = line[1..].trim_end_matches('\n').as_bytes().to_vec();
    let mut data = Vec::new();
    for group in body.chunks(4) {
        if group.len() < 4 {
            break;
        }
        let a = uu_value_byte(group[0]);
        let b = uu_value_byte(group[1]);
        let c = uu_value_byte(group[2]);
        let d = uu_value_byte(group[3]);
        data.push((a << 2) | (b >> 4));
        data.push(((b & 0xf) << 4) | (c >> 2));
        data.push(((c & 0x3) << 6) | d);
    }
    data.truncate(len);
    data
}

fn uu_encode_chunk_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("uu._uu_encode_chunk", args, 1)?;
    let data = bytes_from_obj(&args[0]).unwrap_or_else(|| args[0].py_to_string().into_bytes());
    Ok(PyObject::str_val(CompactString::from(uu_encode_chunk(
        &data,
    ))))
}

fn uu_decode_line_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("uu._uu_decode_line", args, 1)?;
    Ok(PyObject::bytes(uu_decode_line_bytes(
        &args[0].py_to_string(),
    )))
}

fn uu_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "encode requires in_file and out_file",
        ));
    }
    let data_obj = read_file_or_object(&args[0], None)?;
    let data = bytes_from_obj(&data_obj).unwrap_or_else(|| data_obj.py_to_string().into_bytes());
    let name = args
        .get(2)
        .filter(|obj| !matches!(obj.payload, PyObjectPayload::None))
        .map(|obj| obj.py_to_string())
        .unwrap_or_else(|| "-".to_string());
    let mode = args
        .get(3)
        .filter(|obj| !matches!(obj.payload, PyObjectPayload::None))
        .map(|obj| obj.to_int().unwrap_or(0o666))
        .unwrap_or(0o666);
    let mut output = format!("begin {:o} {}\n", mode, name);
    for chunk in data.chunks(45) {
        output.push_str(&uu_encode_chunk(chunk));
    }
    output.push_str(" \nend\n");
    write_file_or_object(
        &args[1],
        PyObject::str_val(CompactString::from(output)),
        false,
    )?;
    Ok(PyObject::none())
}

fn uu_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("decode requires in_file"));
    }
    let text_obj = read_file_or_object(&args[0], None)?;
    let text = text_obj.py_to_string();
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| line.starts_with("begin ")) else {
        return Err(PyException::new(
            ExceptionKind::Exception,
            "No valid begin line found",
        ));
    };
    let mut data = Vec::new();
    for line in lines.iter().skip(start + 1) {
        if *line == "end" {
            break;
        }
        if !line.is_empty() {
            data.extend_from_slice(&uu_decode_line_bytes(line));
        }
    }
    let target = args.get(1).cloned().unwrap_or_else(|| {
        let name = lines[start].splitn(3, ' ').nth(2).unwrap_or("uu.out");
        PyObject::str_val(CompactString::from(name))
    });
    write_file_or_object(&target, PyObject::bytes(data), true)?;
    Ok(PyObject::none())
}
