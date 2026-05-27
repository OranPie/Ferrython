use super::helpers::{arg_or_kw, bool_arg, extract_bytes_like, is_none, split_kwargs};
use super::*;

pub(super) fn base64_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn base64_urlsafe_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(in crate::serial_modules) fn b64_encode_bytes(data: &[u8]) -> Vec<u8> {
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

pub(super) fn base64_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn base64_standard_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn base64_urlsafe_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(in crate::serial_modules) fn b64_decode_bytes(
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

pub(super) fn base64_encodebytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("encodebytes requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, true, "encodebytes")?;
    Ok(PyObject::bytes(legacy_b64_encodebytes(&data)))
}

pub(super) fn base64_decodebytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("decodebytes requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, true, "decodebytes")?;
    Ok(PyObject::bytes(b64_decode_bytes(&data, None, false)?))
}

pub(super) fn base64_encodestring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    crate::introspection_modules::emit_deprecation_warning(
        "encodestring() is deprecated, use encodebytes()",
    );
    base64_encodebytes(args)
}

pub(super) fn base64_decodestring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    crate::introspection_modules::emit_deprecation_warning(
        "decodestring() is deprecated, use decodebytes()",
    );
    base64_decodebytes(args)
}

pub(super) fn legacy_b64_encodebytes(data: &[u8]) -> Vec<u8> {
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

pub(super) fn base64_file_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("encode", args, 2)?;
    let data = read_binary_filelike(&args[0])?;
    write_binary_filelike(&args[1], legacy_b64_encodebytes(&data))?;
    Ok(PyObject::none())
}

pub(super) fn base64_file_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
