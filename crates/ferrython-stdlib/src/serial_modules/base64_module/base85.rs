use super::helpers::{arg_or_kw, bool_arg, extract_bytes_like, int_arg, split_kwargs};
use super::*;

pub(super) fn ascii85_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn ascii85_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn base85_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn base85_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn decode_base85_group(group: &[u8]) -> PyResult<[u8; 4]> {
    let mut value: u64 = 0;
    for &digit in group {
        value = value * 85 + digit as u64;
    }
    if value > u32::MAX as u64 {
        return Err(PyException::value_error("base85 overflow"));
    }
    Ok((value as u32).to_be_bytes())
}

pub(super) fn decode_base85_tail(group: &[u8], out: &mut Vec<u8>) -> PyResult<()> {
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
