use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;

pub fn create_base64_module() -> PyObjectRef {
    make_module(
        "base64",
        vec![
            ("b64encode", make_builtin(base64_encode)),
            ("b64decode", make_builtin(base64_decode)),
            ("encodebytes", make_builtin(base64_encodebytes)),
            ("decodebytes", make_builtin(base64_decodebytes)),
            ("encodestring", make_builtin(base64_encodestring)),
            ("decodestring", make_builtin(base64_decodestring)),
            ("encode", make_builtin(base64_file_encode)),
            ("decode", make_builtin(base64_file_decode)),
            ("b16encode", make_builtin(|args| base16_encode(args))),
            ("b16decode", make_builtin(|args| base16_decode(args))),
            ("b32encode", make_builtin(|args| base32_encode(args))),
            ("b32decode", make_builtin(|args| base32_decode(args))),
            ("urlsafe_b64encode", make_builtin(base64_urlsafe_encode)),
            ("urlsafe_b64decode", make_builtin(base64_urlsafe_decode)),
            ("standard_b64encode", make_builtin(base64_encode)),
            ("standard_b64decode", make_builtin(base64_standard_decode)),
            ("a85encode", make_builtin(ascii85_encode)),
            ("a85decode", make_builtin(ascii85_decode)),
            ("b85encode", make_builtin(base85_encode)),
            ("b85decode", make_builtin(base85_decode)),
        ],
    )
}

pub(crate) fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    extract_bytes_like(obj, true, false, "bytes-like object")
}

fn split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if args.len() > 1 && matches!(&args[args.len() - 1].payload, PyObjectPayload::Dict(_)) {
        (&args[..args.len() - 1], Some(args[args.len() - 1].clone()))
    } else {
        (args, None)
    }
}

fn kw_arg(kwargs: Option<&PyObjectRef>, key: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(key)))
        .cloned()
}

fn arg_or_kw(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
) -> Option<PyObjectRef> {
    pos.get(idx).cloned().or_else(|| kw_arg(kwargs, key))
}

fn bool_arg(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
    default: bool,
) -> bool {
    arg_or_kw(pos, kwargs, idx, key)
        .map(|v| v.is_truthy())
        .unwrap_or(default)
}

fn int_arg(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
    default: i64,
) -> i64 {
    arg_or_kw(pos, kwargs, idx, key)
        .and_then(|v| v.as_int())
        .unwrap_or(default)
}

fn is_none(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::None)
}

fn extract_ascii_str(s: &str) -> PyResult<Vec<u8>> {
    if !s.is_ascii() {
        return Err(PyException::value_error(
            "string argument should contain only ASCII characters",
        ));
    }
    Ok(s.as_bytes().to_vec())
}

pub(super) fn extract_bytes_like(
    obj: &PyObjectRef,
    allow_str: bool,
    legacy_memoryview: bool,
    func_name: &str,
) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        PyObjectPayload::Str(s) if allow_str => extract_ascii_str(s),
        PyObjectPayload::Str(_) => Err(PyException::type_error(format!(
            "{} expected a bytes-like object, not str",
            func_name
        ))),
        PyObjectPayload::Instance(_) if obj.get_attr("__memoryview__").is_some() => {
            if legacy_memoryview {
                let ndim = obj.get_attr("ndim").and_then(|v| v.as_int()).unwrap_or(1);
                let format = obj
                    .get_attr("format")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "B".to_string());
                if ndim != 1 || !matches!(format.as_str(), "B" | "b" | "c") {
                    return Err(PyException::type_error(
                        "expected single-dimensional byte-oriented buffer",
                    ));
                }
            }
            if let Some(base) = obj.get_attr("obj") {
                extract_bytes_like(&base, false, false, func_name)
            } else {
                Err(PyException::type_error("expected bytes-like object"))
            }
        }
        PyObjectPayload::Instance(_) => {
            if let Some(data) = obj.get_attr("_data") {
                if let Some(typecode) = obj.get_attr("typecode") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        return array_items_to_bytes(typecode.py_to_string().as_str(), items);
                    }
                }
            }
            Err(PyException::type_error("expected bytes-like object"))
        }
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

fn array_items_to_bytes(typecode: &str, items: &PyCell<Vec<PyObjectRef>>) -> PyResult<Vec<u8>> {
    let r = items.read();
    let mut out = Vec::new();
    for item in r.iter() {
        let value = item.to_int()?;
        match typecode {
            "b" => out.push(value as i8 as u8),
            "B" => out.push(value as u8),
            "h" => out.extend_from_slice(&(value as i16).to_ne_bytes()),
            "H" => out.extend_from_slice(&(value as u16).to_ne_bytes()),
            "i" | "l" => out.extend_from_slice(&(value as i32).to_ne_bytes()),
            "I" | "L" => out.extend_from_slice(&(value as u32).to_ne_bytes()),
            "q" => out.extend_from_slice(&value.to_ne_bytes()),
            "Q" => out.extend_from_slice(&(value as u64).to_ne_bytes()),
            _ => out.push(value as u8),
        }
    }
    Ok(out)
}

fn base64_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b64encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "b64encode")?;
    let altchars = b64_altchars(pos, kwargs.as_ref(), 1, false)?;
    let mut result = b64_encode_bytes(&data);
    if let Some([plus, slash]) = altchars {
        for b in &mut result {
            if *b == b'+' {
                *b = plus;
            } else if *b == b'/' {
                *b = slash;
            }
        }
    }
    Ok(PyObject::bytes(result))
}

fn base64_urlsafe_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("urlsafe_b64encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "urlsafe_b64encode")?;
    let mut result = b64_encode_bytes(&data);
    for b in &mut result {
        if *b == b'+' {
            *b = b'-';
        } else if *b == b'/' {
            *b = b'_';
        }
    }
    Ok(PyObject::bytes(result))
}

pub(super) fn b64_encode_bytes(data: &[u8]) -> Vec<u8> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize]);
        result.push(CHARS[((n >> 12) & 63) as usize]);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 63) as usize]);
        } else {
            result.push(b'=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize]);
        } else {
            result.push(b'=');
        }
    }
    result
}

fn b64_altchars(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    allow_str: bool,
) -> PyResult<Option<[u8; 2]>> {
    let Some(obj) = arg_or_kw(pos, kwargs, idx, "altchars") else {
        return Ok(None);
    };
    if is_none(&obj) {
        return Ok(None);
    }
    let data = extract_bytes_like(&obj, allow_str, false, "altchars")?;
    if data.len() < 2 {
        return Err(PyException::value_error(
            "altchars must be at least 2 bytes",
        ));
    }
    Ok(Some([data[0], data[1]]))
}

fn base64_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b64decode requires data"));
    }
    let input_bytes = extract_bytes_like(&pos[0], true, false, "b64decode")?;
    let altchars = b64_altchars(pos, kwargs.as_ref(), 1, true)?;
    let validate = bool_arg(pos, kwargs.as_ref(), 2, "validate", false);
    Ok(PyObject::bytes(b64_decode_bytes(
        &input_bytes,
        altchars,
        validate,
    )?))
}

fn base64_standard_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("standard_b64decode requires data"));
    }
    let input_bytes = extract_bytes_like(&pos[0], true, false, "standard_b64decode")?;
    let validate = bool_arg(pos, kwargs.as_ref(), 1, "validate", false);
    Ok(PyObject::bytes(b64_decode_bytes(
        &input_bytes,
        None,
        validate,
    )?))
}

fn base64_urlsafe_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("urlsafe_b64decode requires data"));
    }
    let input_bytes = extract_bytes_like(&pos[0], true, false, "urlsafe_b64decode")?;
    let validate = bool_arg(pos, kwargs.as_ref(), 1, "validate", false);
    Ok(PyObject::bytes(b64_decode_bytes(
        &input_bytes,
        Some([b'-', b'_']),
        validate,
    )?))
}

fn b64_decode_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

pub(super) fn b64_decode_bytes(
    input_bytes: &[u8],
    altchars: Option<[u8; 2]>,
    validate: bool,
) -> PyResult<Vec<u8>> {
    let mut input = Vec::new();
    for &raw in input_bytes {
        let b = match altchars {
            Some([plus, _]) if raw == plus => b'+',
            Some([_, slash]) if raw == slash => b'/',
            _ => raw,
        };
        if b == b'=' || b64_decode_value(b).is_some() {
            input.push(b);
        } else if validate {
            return Err(PyException::value_error("Non-base64 digit found"));
        }
    }
    if input.is_empty() || input.iter().all(|&b| b == b'=') {
        return Ok(Vec::new());
    }
    if input.len() % 4 != 0 {
        return Err(PyException::value_error("Incorrect padding"));
    }
    let first_pad = input.iter().position(|&b| b == b'=');
    if let Some(idx) = first_pad {
        if input[idx..].iter().any(|&b| b != b'=') || input.len() - idx > 2 {
            return Err(PyException::value_error("Incorrect padding"));
        }
    }

    let mut result = Vec::new();
    for chunk in input.chunks(4) {
        if chunk[0] == b'=' || chunk[1] == b'=' {
            return Err(PyException::value_error("Incorrect padding"));
        }
        let v0 = b64_decode_value(chunk[0])
            .ok_or_else(|| PyException::value_error("Non-base64 digit found"))?;
        let v1 = b64_decode_value(chunk[1])
            .ok_or_else(|| PyException::value_error("Non-base64 digit found"))?;
        let v2 = if chunk[2] == b'=' {
            0
        } else {
            b64_decode_value(chunk[2])
                .ok_or_else(|| PyException::value_error("Non-base64 digit found"))?
        };
        let v3 = if chunk[3] == b'=' {
            0
        } else {
            b64_decode_value(chunk[3])
                .ok_or_else(|| PyException::value_error("Non-base64 digit found"))?
        };
        let n = ((v0 as u32) << 18) | ((v1 as u32) << 12) | ((v2 as u32) << 6) | v3 as u32;
        result.push((n >> 16) as u8);
        if chunk[2] != b'=' {
            result.push((n >> 8) as u8);
        }
        if chunk[3] != b'=' {
            result.push(n as u8);
        }
    }
    Ok(result)
}

fn base64_encodebytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("encodebytes requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, true, "encodebytes")?;
    Ok(PyObject::bytes(legacy_b64_encodebytes(&data)))
}

fn base64_decodebytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("decodebytes requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, true, "decodebytes")?;
    Ok(PyObject::bytes(b64_decode_bytes(&data, None, false)?))
}

fn base64_encodestring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    crate::introspection_modules::emit_deprecation_warning(
        "encodestring() is deprecated, use encodebytes()",
    );
    base64_encodebytes(args)
}

fn base64_decodestring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    crate::introspection_modules::emit_deprecation_warning(
        "decodestring() is deprecated, use decodebytes()",
    );
    base64_decodebytes(args)
}

fn legacy_b64_encodebytes(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let encoded = b64_encode_bytes(data);
    let mut out = Vec::with_capacity(encoded.len() + encoded.len() / 76 + 1);
    for chunk in encoded.chunks(76) {
        out.extend_from_slice(chunk);
        out.push(b'\n');
    }
    out
}

fn base64_file_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("encode", args, 2)?;
    let data = read_binary_filelike(&args[0])?;
    write_binary_filelike(&args[1], legacy_b64_encodebytes(&data))?;
    Ok(PyObject::none())
}

fn base64_file_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("decode", args, 2)?;
    let data = read_binary_filelike(&args[0])?;
    write_binary_filelike(&args[1], b64_decode_bytes(&data, None, false)?)?;
    Ok(PyObject::none())
}

fn read_binary_filelike(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    if obj.get_attr("__stringio__").is_some() {
        return Err(PyException::type_error("expected binary file object"));
    }
    let read = obj
        .get_attr("read")
        .ok_or_else(|| PyException::type_error("expected file object with read()"))?;
    let data = call_native_file_method(&read, obj, &[])?;
    match &data.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        _ => Err(PyException::type_error("expected binary file object")),
    }
}

fn write_binary_filelike(obj: &PyObjectRef, data: Vec<u8>) -> PyResult<()> {
    if obj.get_attr("__stringio__").is_some() {
        return Err(PyException::type_error("expected binary file object"));
    }
    let write = obj
        .get_attr("write")
        .ok_or_else(|| PyException::type_error("expected file object with write()"))?;
    call_native_file_method(&write, obj, &[PyObject::bytes(data)])?;
    Ok(())
}

fn call_native_file_method(
    method: &PyObjectRef,
    receiver: &PyObjectRef,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match &method.payload {
        PyObjectPayload::NativeClosure(nc) => (nc.func)(args),
        PyObjectPayload::NativeFunction(nf) => (nf.func)(args),
        PyObjectPayload::BoundMethod { method, .. } => {
            let mut full_args = vec![receiver.clone()];
            full_args.extend_from_slice(args);
            match &method.payload {
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&full_args),
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&full_args),
                _ => Err(PyException::type_error("file method requires VM dispatch")),
            }
        }
        _ => Err(PyException::type_error("file method requires VM dispatch")),
    }
}

fn base16_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b16encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "b16encode")?;
    let hex: String = data.iter().map(|b| format!("{:02X}", b)).collect();
    Ok(PyObject::bytes(hex.into_bytes()))
}

fn hex_value(b: u8, casefold: bool) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' if casefold => Some(b - b'a' + 10),
        _ => None,
    }
}

fn base16_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b16decode requires data"));
    }
    let casefold = bool_arg(pos, kwargs.as_ref(), 1, "casefold", false);
    let input = extract_bytes_like(&pos[0], true, false, "b16decode")?;
    if input.len() % 2 != 0 {
        return Err(PyException::value_error("Odd-length string"));
    }
    let mut result = Vec::with_capacity(input.len() / 2);
    for pair in input.chunks(2) {
        let hi = hex_value(pair[0], casefold)
            .ok_or_else(|| PyException::value_error("Non-base16 digit found"))?;
        let lo = hex_value(pair[1], casefold)
            .ok_or_else(|| PyException::value_error("Non-base16 digit found"))?;
        result.push((hi << 4) | lo);
    }
    Ok(PyObject::bytes(result))
}

fn base32_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b32encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "b32encode")?;
    Ok(PyObject::bytes(base32_encode_bytes(&data)))
}

fn base32_encode_bytes(data: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = Vec::new();
    for chunk in data.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let b = buf;
        let indices = [
            (b[0] >> 3) & 0x1F,
            ((b[0] & 0x07) << 2) | ((b[1] >> 6) & 0x03),
            (b[1] >> 1) & 0x1F,
            ((b[1] & 0x01) << 4) | ((b[2] >> 4) & 0x0F),
            ((b[2] & 0x0F) << 1) | ((b[3] >> 7) & 0x01),
            (b[3] >> 2) & 0x1F,
            ((b[3] & 0x03) << 3) | ((b[4] >> 5) & 0x07),
            b[4] & 0x1F,
        ];
        let num_chars = match chunk.len() {
            1 => 2,
            2 => 4,
            3 => 5,
            4 => 7,
            5 => 8,
            _ => 0,
        };
        for i in 0..num_chars {
            result.push(ALPHABET[indices[i] as usize]);
        }
        for _ in 0..(8 - num_chars) {
            result.push(b'=');
        }
    }
    result
}

fn base32_value(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'2'..=b'7' => Some(b - b'2' + 26),
        _ => None,
    }
}

fn base32_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b32decode requires data"));
    }
    let casefold = bool_arg(pos, kwargs.as_ref(), 1, "casefold", false);
    let map01 = arg_or_kw(pos, kwargs.as_ref(), 2, "map01")
        .filter(|v| !is_none(v))
        .map(|v| extract_bytes_like(&v, true, false, "map01"))
        .transpose()?
        .and_then(|v| v.first().copied());
    let mut input = extract_bytes_like(&pos[0], true, false, "b32decode")?;
    if input.is_empty() {
        return Ok(PyObject::bytes(Vec::new()));
    }
    if input.len() % 8 != 0 {
        return Err(PyException::value_error("Incorrect padding"));
    }
    for b in &mut input {
        if *b == b'0' {
            if map01.is_some() {
                *b = b'O';
            }
        } else if *b == b'1' {
            if let Some(m) = map01 {
                *b = m.to_ascii_uppercase();
            }
        } else if b.is_ascii_lowercase() {
            if casefold {
                *b = b.to_ascii_uppercase();
            }
        }
    }

    let mut result = Vec::new();
    for chunk in input.chunks(8) {
        let pad_count = chunk.iter().filter(|&&b| b == b'=').count();
        if !matches!(pad_count, 0 | 1 | 3 | 4 | 6) {
            return Err(PyException::value_error("Incorrect padding"));
        }
        if pad_count > 0 && chunk[8 - pad_count..].iter().any(|&b| b != b'=') {
            return Err(PyException::value_error("Incorrect padding"));
        }
        let mut vals = [0u8; 8];
        for i in 0..8 - pad_count {
            vals[i] = base32_value(chunk[i])
                .ok_or_else(|| PyException::value_error("Non-base32 digit found"))?;
        }
        let n = ((vals[0] as u64) << 35)
            | ((vals[1] as u64) << 30)
            | ((vals[2] as u64) << 25)
            | ((vals[3] as u64) << 20)
            | ((vals[4] as u64) << 15)
            | ((vals[5] as u64) << 10)
            | ((vals[6] as u64) << 5)
            | (vals[7] as u64);
        let out_bytes = match pad_count {
            6 => 1,
            4 => 2,
            3 => 3,
            1 => 4,
            0 => 5,
            _ => unreachable!(),
        };
        if out_bytes >= 1 {
            result.push((n >> 32) as u8);
        }
        if out_bytes >= 2 {
            result.push((n >> 24) as u8);
        }
        if out_bytes >= 3 {
            result.push((n >> 16) as u8);
        }
        if out_bytes >= 4 {
            result.push((n >> 8) as u8);
        }
        if out_bytes >= 5 {
            result.push(n as u8);
        }
    }
    Ok(PyObject::bytes(result))
}

fn ascii85_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("a85encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "a85encode")?;
    let foldspaces = bool_arg(pos, kwargs.as_ref(), 1, "foldspaces", false);
    let wrapcol = int_arg(pos, kwargs.as_ref(), 2, "wrapcol", 0);
    let pad = bool_arg(pos, kwargs.as_ref(), 3, "pad", false);
    let adobe = bool_arg(pos, kwargs.as_ref(), 4, "adobe", false);
    let body = ascii85_encode_bytes(&data, foldspaces, pad);
    Ok(PyObject::bytes(wrap_ascii85(body, wrapcol, adobe)))
}

fn ascii85_encode_bytes(data: &[u8], foldspaces: bool, pad: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let len = chunk.len();
        let mut buf = [0u8; 4];
        buf[..len].copy_from_slice(chunk);
        if len == 4 && buf == [0, 0, 0, 0] {
            out.push(b'z');
            continue;
        }
        if len == 4 && foldspaces && buf == [b' ', b' ', b' ', b' '] {
            out.push(b'y');
            continue;
        }
        let mut chars = [0u8; 5];
        let mut value = u32::from_be_bytes(buf);
        for i in (0..5).rev() {
            chars[i] = (value % 85) as u8 + 33;
            value /= 85;
        }
        let take = if len == 4 || pad { 5 } else { len + 1 };
        out.extend_from_slice(&chars[..take]);
    }
    out
}

fn wrap_ascii85(body: Vec<u8>, wrapcol: i64, adobe: bool) -> Vec<u8> {
    if wrapcol <= 0 {
        let mut out = Vec::new();
        if adobe {
            out.extend_from_slice(b"<~");
        }
        out.extend_from_slice(&body);
        if adobe {
            out.extend_from_slice(b"~>");
        }
        return out;
    }
    let wrap = wrapcol as usize;
    let mut source = Vec::new();
    if adobe {
        source.extend_from_slice(b"<~");
    }
    source.extend_from_slice(&body);
    let mut out = Vec::new();
    for (idx, chunk) in source.chunks(wrap).enumerate() {
        if idx > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(chunk);
    }
    if adobe {
        if !out.is_empty() {
            out.push(b'\n');
        }
        out.extend_from_slice(b"~>");
    }
    out
}

fn ascii85_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("a85decode requires data"));
    }
    let data = extract_bytes_like(&pos[0], true, false, "a85decode")?;
    let foldspaces = bool_arg(pos, kwargs.as_ref(), 1, "foldspaces", false);
    let adobe = bool_arg(pos, kwargs.as_ref(), 2, "adobe", false);
    let ignorechars = arg_or_kw(pos, kwargs.as_ref(), 3, "ignorechars")
        .map(|v| extract_bytes_like(&v, false, false, "ignorechars"))
        .transpose()?
        .unwrap_or_else(|| b" \t\n\r\x0b".to_vec());
    Ok(PyObject::bytes(ascii85_decode_bytes(
        &data,
        foldspaces,
        adobe,
        &ignorechars,
    )?))
}

fn ascii85_decode_bytes(
    data: &[u8],
    foldspaces: bool,
    adobe: bool,
    ignorechars: &[u8],
) -> PyResult<Vec<u8>> {
    let mut input = data;
    if adobe {
        if input.starts_with(b"<~") {
            input = &input[2..];
        }
        if !input.ends_with(b"~>") {
            return Err(PyException::value_error(
                "Ascii85 encoded byte sequences must end with b'~>'",
            ));
        }
        input = &input[..input.len() - 2];
    } else if input.windows(2).any(|w| w == b"<~" || w == b"~>") {
        return Err(PyException::value_error(
            "Ascii85 adobe markers are not allowed",
        ));
    }
    let mut out = Vec::new();
    let mut group: Vec<u8> = Vec::with_capacity(5);
    for &b in input {
        if ignorechars.contains(&b) {
            continue;
        }
        if b == b'z' {
            if !group.is_empty() {
                return Err(PyException::value_error("z inside Ascii85 5-tuple"));
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if b == b'y' {
            if !foldspaces || !group.is_empty() {
                return Err(PyException::value_error("y inside Ascii85 5-tuple"));
            }
            out.extend_from_slice(b"    ");
            continue;
        }
        if !(33..=117).contains(&b) {
            return Err(PyException::value_error("Non-Ascii85 digit found"));
        }
        group.push(b - 33);
        if group.len() == 5 {
            out.extend_from_slice(&decode_base85_group(&group)?);
            group.clear();
        }
    }
    decode_base85_tail(&group, &mut out)?;
    Ok(out)
}

fn base85_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b85encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "b85encode")?;
    let pad = bool_arg(pos, kwargs.as_ref(), 1, "pad", false);
    Ok(PyObject::bytes(base85_encode_bytes(&data, pad)))
}

const B85_ALPHABET: &[u8; 85] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

fn base85_encode_bytes(data: &[u8], pad: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let len = chunk.len();
        let mut buf = [0u8; 4];
        buf[..len].copy_from_slice(chunk);
        let mut chars = [0u8; 5];
        let mut value = u32::from_be_bytes(buf);
        for i in (0..5).rev() {
            chars[i] = B85_ALPHABET[(value % 85) as usize];
            value /= 85;
        }
        let take = if len == 4 || pad { 5 } else { len + 1 };
        out.extend_from_slice(&chars[..take]);
    }
    out
}

fn base85_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b85decode requires data"));
    }
    let data = extract_bytes_like(&pos[0], true, false, "b85decode")?;
    Ok(PyObject::bytes(base85_decode_bytes(&data)?))
}

fn base85_decode_bytes(data: &[u8]) -> PyResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut group: Vec<u8> = Vec::with_capacity(5);
    for &b in data {
        let Some(pos) = B85_ALPHABET.iter().position(|&x| x == b) else {
            return Err(PyException::value_error("bad base85 character"));
        };
        group.push(pos as u8);
        if group.len() == 5 {
            out.extend_from_slice(&decode_base85_group(&group)?);
            group.clear();
        }
    }
    decode_base85_tail(&group, &mut out)?;
    Ok(out)
}

fn decode_base85_group(group: &[u8]) -> PyResult<[u8; 4]> {
    let mut value: u64 = 0;
    for &digit in group {
        value = value * 85 + digit as u64;
    }
    if value > u32::MAX as u64 {
        return Err(PyException::value_error("base85 overflow"));
    }
    Ok((value as u32).to_be_bytes())
}

fn decode_base85_tail(group: &[u8], out: &mut Vec<u8>) -> PyResult<()> {
    if group.is_empty() {
        return Ok(());
    }
    if group.len() == 1 {
        return Err(PyException::value_error("base85 length is invalid"));
    }
    let mut padded = group.to_vec();
    while padded.len() < 5 {
        padded.push(84);
    }
    let bytes = decode_base85_group(&padded)?;
    out.extend_from_slice(&bytes[..group.len() - 1]);
    Ok(())
}
